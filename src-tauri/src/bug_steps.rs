use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageFormat, ImageReader, Limits};
use scraper::{ElementRef, Html};
use serde::Deserialize;
use std::io::Cursor;

const MAX_HTML: usize = 64 * 1024;
const MAX_IMAGE: usize = 5 * 1024 * 1024;
const MAX_IMAGES: usize = 8;
const MAX_TOTAL_IMAGES: usize = 12 * 1024 * 1024;
const MAX_TAG_STARTS: usize = 4096;
// 与 image_actions 的预览和导出使用相同的解码尺寸限制。
const MAX_SIDE: u32 = 16_384;
const MAX_PIXELS: u64 = 40_000_000;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum StepsFormat {
    Html,
}

pub(crate) struct StepImage {
    pub(crate) bytes: Vec<u8>,
    pub(crate) mime: &'static str,
    pub(crate) extension: &'static str,
    alt: Option<String>,
}

impl StepImage {
    fn parse(source: &str, alt: Option<&str>, remaining: usize) -> Result<Self, String> {
        let (header, base64) = source.split_once(',').ok_or("图片数据格式无效")?;
        let (mime, extension, format) = match header.to_ascii_lowercase().as_str() {
            "data:image/png;base64" => ("image/png", "png", ImageFormat::Png),
            "data:image/jpeg;base64" => ("image/jpeg", "jpg", ImageFormat::Jpeg),
            "data:image/gif;base64" => ("image/gif", "gif", ImageFormat::Gif),
            "data:image/webp;base64" => ("image/webp", "webp", ImageFormat::WebP),
            _ => return Err("复现步骤只支持 PNG、JPEG、GIF 和 WebP 图片".into()),
        };
        let limit = MAX_IMAGE.min(remaining);
        let too_large = "单张图片最多 5 MB，复现步骤的图片合计最多 12 MB";
        if base64.len() > limit.div_ceil(3) * 4 {
            return Err(too_large.into());
        }
        let bytes = STANDARD.decode(base64).map_err(|_| "图片编码无效")?;
        if bytes.len() > limit {
            return Err(too_large.into());
        }
        if image::guess_format(&bytes).ok() != Some(format) {
            return Err("图片内容与声明格式不一致".into());
        }
        let (width, height) = ImageReader::with_format(Cursor::new(&bytes), format)
            .into_dimensions()
            .map_err(|_| "图片内容损坏")?;
        if width == 0
            || height == 0
            || width > MAX_SIDE
            || height > MAX_SIDE
            || u64::from(width) * u64::from(height) > MAX_PIXELS
        {
            return Err("图片尺寸过大或无效".into());
        }
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_SIDE);
        limits.max_image_height = Some(MAX_SIDE);
        limits.max_alloc = Some(MAX_PIXELS * 8);
        let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
        reader.limits(limits);
        reader.decode().map_err(|_| "图片解码失败")?;
        Ok(Self {
            bytes,
            mime,
            extension,
            alt: alt.map(str::to_owned),
        })
    }
}

pub(crate) struct HtmlSteps {
    // 图片节点独立于 HTML 片段，替换时不匹配或修改用户输入的文本。
    chunks: Vec<String>,
    pub(crate) images: Vec<StepImage>,
    image_bytes: usize,
}

impl HtmlSteps {
    pub(crate) fn parse(input: &str) -> Result<Self, String> {
        let max_input = MAX_TOTAL_IMAGES.div_ceil(3) * 4 + MAX_IMAGES * 64 + MAX_HTML;
        if input.len() > max_input || input.contains('\0') {
            return Err("复现步骤过长或包含无效字符".into());
        }
        if input
            .bytes()
            .filter(|&byte| byte == b'<')
            .take(MAX_TAG_STARTS + 1)
            .count()
            > MAX_TAG_STARTS
        {
            return Err("复现步骤包含过多 HTML 标签".into());
        }
        let fragment = Html::parse_fragment(input);
        let mut result = Self {
            chunks: vec![String::new()],
            images: Vec::new(),
            image_bytes: 0,
        };
        result.children(fragment.root_element(), 0)?;
        if result.render(|_, _| String::new()).len() > MAX_HTML {
            return Err("复现步骤的文字和格式超过 64 KB".into());
        }
        Ok(result)
    }

    pub(crate) fn render(&self, mut source: impl FnMut(usize, &StepImage) -> String) -> String {
        let mut html = self.chunks[0].clone();
        for (index, image) in self.images.iter().enumerate() {
            html.push_str("<img");
            attribute(&mut html, "src", &source(index, image));
            if let Some(alt) = &image.alt {
                attribute(&mut html, "alt", alt);
            }
            html.push('>');
            html.push_str(&self.chunks[index + 1]);
        }
        html
    }

    pub(crate) fn local_html(&self) -> String {
        self.render(|_, image| {
            format!(
                "data:{};base64,{}",
                image.mime,
                STANDARD.encode(&image.bytes)
            )
        })
    }

    fn output(&mut self) -> &mut String {
        self.chunks.last_mut().expect("至少有一个 HTML 片段")
    }

    fn children(&mut self, parent: ElementRef<'_>, depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err("复现步骤的 HTML 嵌套过深".into());
        }
        for node in parent.children() {
            if let Some(text) = node.value().as_text() {
                escape(self.output(), text);
            } else if let Some(element) = ElementRef::wrap(node) {
                self.element(element, depth)?;
            }
        }
        Ok(())
    }

    fn element(&mut self, element: ElementRef<'_>, depth: usize) -> Result<(), String> {
        let tag = element.value().name();
        if element.value().name.ns.as_ref() != "http://www.w3.org/1999/xhtml"
            || matches!(
                tag,
                "script" | "style" | "iframe" | "object" | "template" | "noscript"
            )
        {
            return Ok(());
        }
        if tag == "img" {
            if let Some(source) = element.value().attr("src").filter(|src| {
                src.get(..5)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
            }) {
                if self.images.len() == MAX_IMAGES {
                    return Err("复现步骤最多粘贴 8 张图片".into());
                }
                let image = StepImage::parse(
                    source,
                    element.value().attr("alt"),
                    MAX_TOTAL_IMAGES - self.image_bytes,
                )?;
                self.image_bytes += image.bytes.len();
                self.images.push(image);
                self.chunks.push(String::new());
            }
            return Ok(());
        }
        if !matches!(
            tag,
            "p" | "br"
                | "div"
                | "span"
                | "strong"
                | "b"
                | "em"
                | "i"
                | "u"
                | "s"
                | "strike"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "ul"
                | "ol"
                | "li"
                | "blockquote"
                | "pre"
                | "code"
                | "hr"
                | "a"
                | "table"
                | "thead"
                | "tbody"
                | "tfoot"
                | "tr"
                | "th"
                | "td"
        ) {
            return self.children(element, depth + 1);
        }
        let output = self.output();
        output.push('<');
        output.push_str(tag);
        if tag == "a" {
            if let Some(href) = element.value().attr("href").filter(|href| {
                url::Url::parse(href)
                    .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.has_host())
            }) {
                attribute(output, "href", href);
            }
        } else if tag == "td" || tag == "th" {
            for name in ["colspan", "rowspan"] {
                if let Some(count) = element
                    .value()
                    .attr(name)
                    .and_then(|n| n.parse::<u32>().ok())
                    .filter(|n| (1..=100).contains(n))
                {
                    attribute(output, name, &count.to_string());
                }
            }
        } else if tag == "ol" {
            if let Some(start) = element
                .value()
                .attr("start")
                .and_then(|n| n.parse::<i32>().ok())
            {
                attribute(output, "start", &start.to_string());
            }
        }
        output.push('>');
        if !matches!(tag, "br" | "hr") {
            self.children(element, depth + 1)?;
            let output = self.output();
            output.push_str("</");
            output.push_str(tag);
            output.push('>');
        }
        Ok(())
    }
}

fn attribute(output: &mut String, name: &str, value: &str) {
    output.push(' ');
    output.push_str(name);
    output.push_str("=\"");
    escape(output, value);
    output.push('"');
}

fn escape(output: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(c),
        }
    }
}

#[cfg(test)]
pub(crate) fn fixture_png() -> Vec<u8> {
    let mut image = Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(1, 1)
        .write_to(&mut image, ImageFormat::Png)
        .unwrap();
    image.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_task_markup_and_reserializes_only_allowed_attributes() {
        let html = r#"<p onclick="bad()"><strong>复现</strong> &amp; <u>检查</u></p>
<ol start="3"><li><p>点击</p></li></ol><table><tr><th colspan="2" style="color:red">结果</th><td rowspan="0">失败</td></tr></table>
<a href="javascript:bad()">危险链接</a><a href="https://example.test/?a=1&amp;b=2" target="_blank">查看</a>
<script>hidden()</script><svg><image href="https://external.invalid/x"></image></svg><img src="https://external.invalid/y">"#;
        let steps = HtmlSteps::parse(html).unwrap();
        assert!(steps.images.is_empty());
        assert_eq!(steps.local_html(), concat!(
            "<p><strong>复现</strong> &amp; <u>检查</u></p>\n",
            "<ol start=\"3\"><li><p>点击</p></li></ol><table><tbody><tr><th colspan=\"2\">结果</th><td>失败</td></tr></tbody></table>\n",
            "<a>危险链接</a><a href=\"https://example.test/?a=1&amp;b=2\">查看</a>\n",
        ));
    }

    #[test]
    fn validates_image_mime_decoding_and_limits() {
        for (mime, format) in [
            ("image/png", ImageFormat::Png),
            ("image/jpeg", ImageFormat::Jpeg),
            ("image/gif", ImageFormat::Gif),
            ("image/webp", ImageFormat::WebP),
        ] {
            let mut buffer = Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(2, 2)
                .write_to(&mut buffer, format)
                .unwrap();
            let data = STANDARD.encode(buffer.into_inner());
            let markup =
                format!(r#"<img src="data:{mime};base64,{data}" alt="&quot; onerror=bad()">"#);
            let parsed = HtmlSteps::parse(&markup).unwrap();
            assert_eq!(parsed.images[0].mime, mime);
            assert_eq!(parsed.local_html(), markup);
        }
        let png = STANDARD.encode(fixture_png());
        let source = format!("data:image/png;base64,{png}");
        assert!(StepImage::parse(&source, None, 1).is_err());
        assert!(
            StepImage::parse(&format!("data:image/jpeg;base64,{png}"), None, MAX_IMAGE).is_err()
        );
        assert!(StepImage::parse("data:image/png;base64,iVBORw0KGgo=", None, MAX_IMAGE).is_err());
        assert!(StepImage::parse("data:image/gif;base64,%%", None, MAX_IMAGE).is_err());
        assert!(
            HtmlSteps::parse(&format!(r#"<img src="{source}">"#).repeat(MAX_IMAGES + 1)).is_err()
        );
    }
}
