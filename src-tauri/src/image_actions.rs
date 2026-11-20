use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageFormat, ImageReader, Limits, RgbaImage};
use std::{borrow::Cow, io::Cursor, path::Path, sync::Mutex};
use tauri_plugin_dialog::DialogExt;

const MAX_BYTES: usize = 12 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;
const MAX_PIXELS: u64 = 40_000_000;
// 保持原生剪贴板对象存活，避免 Linux 下剪贴板内容随对象销毁而丢失。
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

struct ValidatedImage {
    bytes: Vec<u8>,
    pixels: RgbaImage,
    extension: &'static str,
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err("图片尺寸过大或无效".into());
    }
    Ok(())
}

fn parse_image(data_url: &str) -> Result<ValidatedImage, String> {
    let (header, encoded) = data_url.split_once(',').ok_or("图片数据格式无效")?;
    let (format, extension) = match header {
        "data:image/png;base64" => (ImageFormat::Png, "png"),
        "data:image/jpeg;base64" => (ImageFormat::Jpeg, "jpg"),
        "data:image/gif;base64" => (ImageFormat::Gif, "gif"),
        "data:image/webp;base64" => (ImageFormat::WebP, "webp"),
        _ => return Err("只支持已加载的 PNG、JPEG、GIF、WebP 图片".into()),
    };
    if encoded.len() > MAX_BYTES.div_ceil(3) * 4 {
        return Err("图片不能超过 12 MiB".into());
    }
    let bytes = STANDARD.decode(encoded).map_err(|_| "图片编码无效")?;
    if bytes.len() > MAX_BYTES {
        return Err("图片不能超过 12 MiB".into());
    }
    if image::guess_format(&bytes).map_err(|_| "图片格式无效")? != format {
        return Err("图片内容与声明格式不一致".into());
    }
    let (width, height) = ImageReader::with_format(Cursor::new(&bytes), format)
        .into_dimensions()
        .map_err(|_| "图片内容损坏")?;
    validate_dimensions(width, height)?;
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    let pixels = reader
        .decode()
        .map_err(|_| "图片解码失败或尺寸过大")?
        .into_rgba8();
    Ok(ValidatedImage {
        bytes,
        pixels,
        extension,
    })
}

fn save_image(path: &Path, image: &ValidatedImage) -> Result<(), String> {
    std::fs::write(path, &image.bytes).map_err(|e| format!("保存图片失败：{e}"))
}

fn with_clipboard(
    operation: impl FnOnce(&mut arboard::Clipboard) -> Result<(), String>,
) -> Result<(), String> {
    let mut clipboard = CLIPBOARD.lock().map_err(|_| "剪贴板暂时不可用")?;
    if clipboard.is_none() {
        *clipboard = Some(arboard::Clipboard::new().map_err(|e| format!("无法打开剪贴板：{e}"))?);
    }
    operation(clipboard.as_mut().ok_or("剪贴板暂时不可用")?)
}

pub(crate) fn write_clipboard_text(value: String) -> Result<(), String> {
    with_clipboard(|clipboard| {
        clipboard
            .set_text(value)
            .map_err(|_| "无法写入剪贴板。".into())
    })
}

#[tauri::command]
pub async fn image_clipboard_write(data_url: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let image = parse_image(&data_url)?;
        with_clipboard(|clipboard| {
            clipboard
                .set_image(arboard::ImageData {
                    width: image.pixels.width() as usize,
                    height: image.pixels.height() as usize,
                    bytes: Cow::Owned(image.pixels.into_raw()),
                })
                .map_err(|e| format!("复制图片失败：{e}"))
        })
    })
    .await
    .map_err(|_| "复制图片任务失败".to_string())?
}

#[tauri::command]
pub async fn image_download(app: tauri::AppHandle, data_url: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let image = parse_image(&data_url)?;
        let selected = app
            .dialog()
            .file()
            .set_title("保存图片")
            .set_file_name(format!("图片.{}", image.extension))
            .add_filter("图片", &[image.extension])
            .blocking_save_file();
        let Some(selected) = selected else {
            return Ok(false);
        };
        let path = selected.into_path().map_err(|_| "请选择本地文件保存位置")?;
        save_image(&path, &image)?;
        Ok(true)
    })
    .await
    .map_err(|_| "保存图片任务失败".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(format: ImageFormat) -> String {
        let mut bytes = Cursor::new(Vec::new());
        let image = image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            2,
            3,
            image::Rgba([23, 44, 55, 255]),
        ));
        let image = if format == ImageFormat::Jpeg {
            image::DynamicImage::ImageRgb8(image.into_rgb8())
        } else {
            image
        };
        image.write_to(&mut bytes, format).unwrap();
        let mime = match format {
            ImageFormat::Png => "png",
            ImageFormat::Gif => "gif",
            ImageFormat::WebP => "webp",
            ImageFormat::Jpeg => "jpeg",
            _ => unreachable!(),
        };
        format!(
            "data:image/{mime};base64,{}",
            STANDARD.encode(bytes.into_inner())
        )
    }
    #[test]
    fn accepts_raster_and_preserves_original_bytes_on_save() {
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::WebP,
        ] {
            let data = fixture(format);
            let image = parse_image(&data).unwrap();
            assert_eq!(image.pixels.dimensions(), (2, 3));
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("image.{}", image.extension));
            save_image(&path, &image).unwrap();
            assert_eq!(std::fs::read(path).unwrap(), image.bytes);
        }
    }
    #[test]
    fn rejects_urls_svg_mismatch_bad_encoding_and_truncated_images() {
        for invalid in [
            "https://example.com/a.png",
            "data:image/svg+xml;base64,PHN2Zz4=",
            "data:image/png;base64,%%%",
            "data:image/png;base64,iVBORw0KGgo=",
        ] {
            assert!(parse_image(invalid).is_err());
        }
        assert!(
            parse_image(&fixture(ImageFormat::Png).replace("image/png", "image/jpeg")).is_err()
        );
    }
    #[test]
    fn rejects_encoded_and_decoded_size_limits() {
        assert!(parse_image(&format!(
            "data:image/png;base64,{}",
            "A".repeat(MAX_BYTES.div_ceil(3) * 4 + 1)
        ))
        .is_err());
        assert!(validate_dimensions(0, 3).is_err());
        assert!(validate_dimensions(MAX_DIMENSION + 1, 1).is_err());
        assert!(validate_dimensions(10_000, 10_000).is_err());
        assert!(validate_dimensions(2, 3).is_ok());
    }
    #[test]
    fn save_failure_is_reported() {
        let image = parse_image(&fixture(ImageFormat::Png)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        assert!(save_image(dir.path(), &image).is_err());
    }
}
