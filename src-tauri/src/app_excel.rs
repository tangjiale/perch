use crate::bookmark_apps::application_url;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use calamine::{DataType, Reader, SheetType, Xls, Xlsx};
use serde::Serialize;
use std::{
    borrow::Cow,
    fmt,
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
};

pub(crate) const MAX_EXCEL_ROWS: usize = 1_000;
pub(crate) const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;
const MAX_ENCODED_BYTES: usize = MAX_FILE_BYTES.div_ceil(3) * 4;
const MAX_XLSX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_XLSX_MEMBERS: usize = 2_048;
const MAX_XLS_CELLS: usize = 500_000;
const MAX_FIELD_BYTES: usize = 128 * 1024;
const MAX_PREVIEW_BYTES: usize = 5 * 1024 * 1024;
const COLUMN_NAMES: [&str; 6] = ["名称", "链接地址", "分类", "描述信息", "账号", "密码"];
const REQUIRED_COLUMNS: usize = 4;
const PASSWORD_COLUMN: usize = 5;
const INVALID_FILE: &str = "文件格式与扩展名不符，或 Excel 文件已损坏。";
const INVALID_SHEET: &str = "无法读取首张工作表，请检查文件是否完整。";
const LARGE_SHEET: &str = "Excel 工作表数据范围过大，请精简工作簿后重试。";
const LARGE_FILE: &str = "Excel 文件大小不能超过 5 MiB。";
const TEMPLATE: &[u8] = include_bytes!("../../public/templates/applications.xlsx");

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppExcelRow {
    pub row: usize,
    pub name: String,
    pub url: String,
    pub category: String,
    pub description: String,
    #[serde(skip_serializing)]
    pub account: String,
    #[serde(skip_serializing)]
    pub password: String,
}

impl fmt::Debug for AppExcelRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppExcelRow")
            .field("row", &self.row)
            .field("name", &self.name)
            .field("url", &self.url)
            .field("category", &self.category)
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppExcelError {
    pub row: usize,
    pub message: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppExcelPreview {
    pub rows: Vec<AppExcelRow>,
    pub errors: Vec<AppExcelError>,
}

#[tauri::command]
pub async fn preview_app_excel(
    file_name: String,
    contents: String,
) -> Result<AppExcelPreview, String> {
    read_app_excel_file(file_name, contents).await
}

pub(crate) async fn read_app_excel_file(
    file_name: String,
    contents: String,
) -> Result<AppExcelPreview, String> {
    if contents.len() > MAX_ENCODED_BYTES {
        return Err(LARGE_FILE.into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = STANDARD
            .decode(contents)
            .map_err(|_| "Excel 文件编码无效。")?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(LARGE_FILE);
        }
        parse_excel(Path::new(&file_name), &bytes)
    })
    .await
    .map_err(|_| "无法解析 Excel 文件。".to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
pub async fn save_app_excel_template(path: PathBuf) -> Result<(), String> {
    if !has_extension(&path, "xlsx") {
        return Err("请选择扩展名为 .xlsx 的模板保存路径。".into());
    }
    tauri::async_runtime::spawn_blocking(move || save_template(&path))
        .await
        .map_err(|_| "无法保存 Excel 模板。".to_owned())?
        .map_err(|_| "无法保存 Excel 模板，请检查目标目录的写入权限。".to_owned())
}

fn save_template(path: &Path) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.write_all(TEMPLATE)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn has_extension(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

fn parse_excel(path: &Path, bytes: &[u8]) -> Result<AppExcelPreview, &'static str> {
    let mut preview = PreviewBuilder::default();
    if has_extension(path, "xlsx") {
        check_xlsx(bytes)?;
        let mut workbook = Xlsx::new(Cursor::new(bytes)).map_err(|_| INVALID_FILE)?;
        let sheet = first_worksheet(&workbook)?;
        let mut cells = workbook
            .worksheet_cells_reader(&sheet)
            .map_err(|_| INVALID_SHEET)?;
        while let Some(cell) = cells.next_cell().map_err(|_| INVALID_SHEET)? {
            let (row, column) = cell.get_position();
            if row >= 1_048_576 || column >= 16_384 {
                return Err(INVALID_SHEET);
            }
            preview.push_cell(row, column, cell.get_value())?;
        }
    } else if has_extension(path, "xls") {
        let checked = check_xls(bytes)?;
        let mut workbook = Xls::new(checked).map_err(|_| INVALID_FILE)?;
        let sheet = first_worksheet(&workbook)?;
        let range = workbook
            .worksheet_range(&sheet)
            .map_err(|_| INVALID_SHEET)?;
        if let Some((first_row, first_col)) = range.start() {
            for (row, column, value) in range.used_cells() {
                preview.push_cell(first_row + row as u32, first_col + column as u32, value)?;
            }
        }
    } else {
        return Err("请选择 .xls 或 .xlsx 文件。");
    }
    preview.finish()
}

fn first_worksheet<R: Read + io::Seek>(workbook: &impl Reader<R>) -> Result<String, &'static str> {
    workbook
        .sheets_metadata()
        .iter()
        .find(|sheet| sheet.typ == SheetType::WorkSheet)
        .map(|sheet| sheet.name.clone())
        .ok_or("Excel 文件没有可导入的工作表。")
}

fn check_xlsx(bytes: &[u8]) -> Result<(), &'static str> {
    if !bytes.starts_with(b"PK\x03\x04") {
        return Err(INVALID_FILE);
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| INVALID_FILE)?;
    if archive.len() > MAX_XLSX_MEMBERS {
        return Err(LARGE_SHEET);
    }
    let mut remaining = MAX_XLSX_BYTES;
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).map_err(|_| INVALID_FILE)?;
        let size = member.size();
        if size > remaining {
            return Err(LARGE_SHEET);
        }
        // 同时检查声明大小和实际解压字节数。超限立即停止。
        if io::copy(&mut member.by_ref().take(size + 1), &mut io::sink())
            .map_err(|_| INVALID_FILE)?
            != size
        {
            return Err(INVALID_FILE);
        }
        remaining -= size;
    }

    check_workbook_type(&mut archive)
}

fn read_package_xml(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<String, &'static str> {
    const MAX_METADATA_BYTES: u64 = 256 * 1024;
    let member = archive.by_name(name).map_err(|_| INVALID_FILE)?;
    if member.size() > MAX_METADATA_BYTES {
        return Err(LARGE_SHEET);
    }
    let mut xml = String::new();
    member
        .take(MAX_METADATA_BYTES + 1)
        .read_to_string(&mut xml)
        .map_err(|_| INVALID_FILE)?;
    Ok(xml)
}

fn parse_package_xml(xml: &str) -> Result<roxmltree::Document<'_>, &'static str> {
    roxmltree::Document::parse_with_options(
        xml.trim_start_matches('\u{feff}'),
        roxmltree::ParsingOptions {
            nodes_limit: 4_096,
            ..Default::default()
        },
    )
    .map_err(|_| INVALID_FILE)
}

fn check_workbook_type(archive: &mut zip::ZipArchive<Cursor<&[u8]>>) -> Result<(), &'static str> {
    const XLSX_MAIN_TYPE: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
    let relationships = read_package_xml(archive, "_rels/.rels")?;
    let relationships = parse_package_xml(&relationships)?;
    let mut documents = relationships.root_element().children().filter(|entry| {
        entry.has_tag_name("Relationship")
            && entry
                .attribute("Type")
                .is_some_and(|kind| kind.ends_with("/relationships/officeDocument"))
    });
    let document = documents.next().ok_or(INVALID_FILE)?;
    if documents.next().is_some() || document.attribute("TargetMode") == Some("External") {
        return Err(INVALID_FILE);
    }
    let target = document.attribute("Target").ok_or(INVALID_FILE)?;
    let target = target.strip_prefix('/').unwrap_or(target);
    let prefix = read_package_xml(archive, "[Content_Types].xml")?;
    let types = parse_package_xml(&prefix)?;

    let mut overrides = types.root_element().children().filter(|part| {
        part.has_tag_name("Override")
            && part
                .attribute("PartName")
                .and_then(|name| name.strip_prefix('/'))
                .is_some_and(|name| name.eq_ignore_ascii_case(target))
    });
    let workbook_type = if let Some(part) = overrides.next() {
        if overrides.next().is_some() {
            return Err(INVALID_FILE);
        }
        part.attribute("ContentType")
    } else if has_extension(Path::new(target), "xml") {
        types
            .root_element()
            .children()
            .find(|part| {
                part.has_tag_name("Default")
                    && part
                        .attribute("Extension")
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
            })
            .and_then(|part| part.attribute("ContentType"))
    } else {
        None
    };
    (workbook_type == Some(XLSX_MAIN_TYPE))
        .then_some(())
        .ok_or(INVALID_FILE)
}

fn check_xls(bytes: &[u8]) -> Result<Cursor<Vec<u8>>, &'static str> {
    let mut container = cfb::CompoundFile::open(Cursor::new(bytes)).map_err(|_| INVALID_FILE)?;
    let stream = if container.is_stream("/Workbook") {
        "/Workbook"
    } else {
        "/Book"
    };
    let mut workbook = Vec::new();
    container
        .open_stream(stream)
        .map_err(|_| INVALID_FILE)?
        .take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut workbook)
        .map_err(|_| INVALID_FILE)?;
    if workbook.len() > MAX_FILE_BYTES {
        return Err(LARGE_SHEET);
    }
    check_biff_ranges(&workbook)?;

    // 只把已核验的工作簿流交给 calamine，防止不同 OLE 解析器选中其他同名流。
    let mut checked =
        cfb::CompoundFile::create(Cursor::new(Vec::new())).map_err(|_| INVALID_FILE)?;
    let mut stream = checked
        .create_stream("/Workbook")
        .map_err(|_| INVALID_FILE)?;
    stream.write_all(&workbook).map_err(|_| INVALID_FILE)?;
    stream.flush().map_err(|_| INVALID_FILE)?;
    drop(stream);
    Ok(checked.into_inner())
}

fn biff_record<'a>(bytes: &mut &'a [u8]) -> Result<(u16, &'a [u8]), &'static str> {
    let (header, body) = bytes.split_at_checked(4).ok_or(INVALID_FILE)?;
    let kind = u16::from_le_bytes([header[0], header[1]]);
    let length = usize::from(u16::from_le_bytes([header[2], header[3]]));
    let (data, rest) = body.split_at_checked(length).ok_or(INVALID_FILE)?;
    *bytes = rest;
    Ok((kind, data))
}

fn check_biff_ranges(workbook: &[u8]) -> Result<(), &'static str> {
    let mut globals = workbook;
    let mut offsets = Vec::new();
    loop {
        match biff_record(&mut globals)? {
            (0x0085, [a, b, c, d, _, _, _, ..]) => {
                offsets.push(u32::from_le_bytes([*a, *b, *c, *d]) as usize);
                if offsets.len() > MAX_XLSX_MEMBERS {
                    return Err(LARGE_SHEET);
                }
            }
            (0x0085, _) | (0x002F, _) => return Err(INVALID_FILE),
            (0x000A, _) => break,
            _ => {}
        }
    }

    // calamine 会为所有 XLS 工作表分配稠密 Range。先限制声明范围和实际坐标跨度。
    offsets.sort_unstable();
    let mut remaining = MAX_XLS_CELLS;
    let mut previous_end = workbook.len() - globals.len();
    for (index, &start) in offsets.iter().enumerate() {
        let end = offsets.get(index + 1).copied().unwrap_or(workbook.len());
        if start < previous_end || start >= end {
            return Err(INVALID_FILE);
        }
        previous_end = end;
        let sheet = workbook.get(start..end).ok_or(INVALID_FILE)?;
        remaining = remaining
            .checked_sub(biff_sheet_area(sheet)?)
            .ok_or(LARGE_SHEET)?;
    }
    Ok(())
}

fn biff_sheet_area(mut sheet: &[u8]) -> Result<usize, &'static str> {
    if !matches!(
        biff_record(&mut sheet)?.0,
        0x0009 | 0x0209 | 0x0409 | 0x0809
    ) {
        return Err(INVALID_FILE);
    }
    let mut nesting = 0;
    let mut declared: usize = 0;
    let mut occupied: Option<[usize; 4]> = None;
    loop {
        let (kind, data) = biff_record(&mut sheet)?;
        match kind {
            0x0809 => nesting += 1,
            0x000A if nesting == 0 => break,
            0x000A => nesting -= 1,
            _ if nesting > 0 => continue,
            0x0200 => {
                declared += biff_dimensions(data)?;
                if declared > MAX_XLS_CELLS {
                    return Err(LARGE_SHEET);
                }
            }
            0x0203 | 0x0204 | 0x00D6 | 0x0205 | 0x027E | 0x00FD | 0x00BD | 0x0006 => {
                let [a, b, c, d, ..] = data else {
                    return Err(INVALID_FILE);
                };
                let row = usize::from(u16::from_le_bytes([*a, *b]));
                let col = usize::from(u16::from_le_bytes([*c, *d]));
                let end_col = if kind == 0x00BD {
                    let [.., a, b] = data.get(4..).ok_or(INVALID_FILE)? else {
                        return Err(INVALID_FILE);
                    };
                    usize::from(u16::from_le_bytes([*a, *b]))
                } else {
                    col
                };
                if end_col < col || end_col >= 256 {
                    return Err(INVALID_SHEET);
                }
                let rect = occupied.get_or_insert([row, row, col, end_col]);
                rect[0] = rect[0].min(row);
                rect[1] = rect[1].max(row);
                rect[2] = rect[2].min(col);
                rect[3] = rect[3].max(end_col);
            }
            _ => {}
        }
    }
    let area = occupied.map_or(0, |[r1, r2, c1, c2]| (r2 - r1 + 1) * (c2 - c1 + 1));
    Ok(declared.max(area))
}

fn biff_dimensions(data: &[u8]) -> Result<usize, &'static str> {
    let (first_row, end_row, first_col, end_col) = match data {
        [a, b, c, d, e, f, g, h, _, _] => (
            u16::from_le_bytes([*a, *b]) as u32,
            u16::from_le_bytes([*c, *d]) as u32,
            u16::from_le_bytes([*e, *f]),
            u16::from_le_bytes([*g, *h]),
        ),
        [a, b, c, d, e, f, g, h, i, j, k, l, ..] if matches!(data.len(), 14 | 16) => (
            u32::from_le_bytes([*a, *b, *c, *d]),
            u32::from_le_bytes([*e, *f, *g, *h]),
            u16::from_le_bytes([*i, *j]),
            u16::from_le_bytes([*k, *l]),
        ),
        _ => return Err(INVALID_SHEET),
    };
    if end_row > 65_536 || end_col > 256 {
        return Err(LARGE_SHEET);
    }
    if end_row == 0 || end_col == 0 {
        return Ok(1);
    }
    if first_row >= end_row || first_col >= end_col {
        return Err(INVALID_SHEET);
    }
    let rows = end_row - first_row;
    let cols = end_col - first_col;
    Ok(rows as usize * usize::from(cols))
}

fn cell_text(value: &impl DataType) -> Cow<'_, str> {
    if let Some(text) = value
        .get_string()
        .or_else(|| value.get_datetime_iso())
        .or_else(|| value.get_duration_iso())
    {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        value
            .as_string()
            .or_else(|| value.get_bool().map(|flag| flag.to_string()))
            .or_else(|| value.get_datetime().map(|date| date.to_string()))
            .or_else(|| value.get_error().map(|error| error.to_string()))
            .unwrap_or_default(),
    )
}

#[derive(Default)]
struct PreviewBuilder {
    result: AppExcelPreview,
    header: [Option<u32>; COLUMN_NAMES.len()],
    header_ready: bool,
    row: Option<u32>,
    fields: [String; COLUMN_NAMES.len()],
    field_error: Option<&'static str>,
    retained_bytes: usize,
}

impl PreviewBuilder {
    fn push_cell(
        &mut self,
        row: u32,
        column: u32,
        value: &impl DataType,
    ) -> Result<(), &'static str> {
        if value.is_empty() {
            return Ok(());
        }
        let text = cell_text(value);
        let trimmed = text.trim();
        // 只对密码列保留纯空白内容，真正的空单元格仍忽略。
        if trimmed.is_empty() && (text.is_empty() || self.header[PASSWORD_COLUMN] != Some(column)) {
            return Ok(());
        }
        if let Some(previous) = self.row {
            if row < previous {
                return Err(INVALID_SHEET);
            }
            if row > previous {
                self.complete_row(previous)?;
            }
        }
        self.row = Some(row);

        if self.header_ready {
            if let Some(index) = self.header.iter().position(|value| *value == Some(column)) {
                let text = if index == PASSWORD_COLUMN {
                    text.as_ref()
                } else {
                    trimmed
                };
                if value.is_error() {
                    self.field_error = Some("包含 Excel 错误单元格，请修正后重试。");
                } else if text.len() > MAX_FIELD_BYTES {
                    self.field_error = Some("单元格内容过长，请缩短后重试。");
                } else {
                    self.fields[index] = text.to_owned();
                }
            }
        } else {
            let title = match trimmed {
                "连接地址" => "链接地址",
                "描述" | "说明" => "描述信息",
                value if value.eq_ignore_ascii_case("URL") => "链接地址",
                _ => trimmed,
            };
            let Some(index) = COLUMN_NAMES.iter().position(|name| *name == title) else {
                return Ok(());
            };
            if self.header.contains(&Some(column)) || self.header[index].replace(column).is_some() {
                return Err("表头中有重复的导入列，请检查并保留一个标题。");
            }
        }
        Ok(())
    }

    fn complete_row(&mut self, row: u32) -> Result<(), &'static str> {
        if !self.header_ready {
            if self.header[..REQUIRED_COLUMNS].contains(&None) {
                return Err("首个非空行必须包含名称、链接地址、分类、描述信息四列表头。");
            }
            self.header_ready = true;
            return Ok(());
        }
        let [name, url, category, description, account, password] =
            std::mem::take(&mut self.fields);
        let field_error = self.field_error.take();
        if field_error.is_none()
            && [&name, &url, &category, &description, &account, &password]
                .iter()
                .all(|s| s.is_empty())
        {
            return Ok(());
        }
        if self.result.rows.len() + self.result.errors.len() >= MAX_EXCEL_ROWS {
            return Err("每次最多预览 1000 条应用，请拆分 Excel 文件后重试。");
        }
        let row = row as usize + 1;
        let required_error = match (name.is_empty(), url.is_empty()) {
            (true, true) => Some("缺少名称和链接地址。"),
            (true, false) => Some("缺少名称。"),
            (false, true) => Some("缺少链接地址。"),
            _ if application_url(&url).is_none() => {
                Some("链接地址须为不含账号密码的 HTTP 或 HTTPS 网址。")
            }
            _ => None,
        };
        if let Some(message) = field_error.or(required_error) {
            self.result.errors.push(AppExcelError {
                row,
                message: message.into(),
            });
        } else {
            self.retained_bytes += name.len()
                + url.len()
                + category.len()
                + description.len()
                + account.len()
                + password.len();
            if self.retained_bytes > MAX_PREVIEW_BYTES {
                return Err(LARGE_SHEET);
            }
            self.result.rows.push(AppExcelRow {
                row,
                name,
                url,
                category,
                description,
                account,
                password,
            });
        }
        Ok(())
    }

    fn finish(mut self) -> Result<AppExcelPreview, &'static str> {
        let row = self.row.ok_or("首张工作表为空，请添加表头。")?;
        self.complete_row(row)?;
        Ok(self.result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::Data;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    fn rewrite_xlsx(replacement: (&str, &[u8]), entry: Option<(&str, &[u8])>) -> Vec<u8> {
        let mut src = zip::ZipArchive::new(Cursor::new(TEMPLATE)).unwrap();
        let mut dest = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for index in 0..src.len() {
            let mut member = src.by_index(index).unwrap();
            dest.start_file(member.name(), opts).unwrap();
            if member.name() == replacement.0 {
                dest.write_all(replacement.1).unwrap();
            } else {
                io::copy(&mut member, &mut dest).unwrap();
            }
        }
        if let Some((name, bytes)) = entry {
            dest.start_file(name, opts).unwrap();
            dest.write_all(bytes).unwrap();
        }
        dest.finish().unwrap().into_inner()
    }

    fn xlsx_with_sheet(sheet_data: &str, entry: Option<(&str, &[u8])>) -> Vec<u8> {
        let sheet = format!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:XFD1048576"/><sheetData>{sheet_data}</sheetData></worksheet>"#
        );
        rewrite_xlsx(("xl/worksheets/sheet1.xml", sheet.as_bytes()), entry)
    }

    #[test]
    fn template_previews_with_no_data_and_saves_exact_embedded_bytes() {
        let preview = parse_excel(Path::new("应用模板.XLSX"), TEMPLATE).unwrap();
        assert!(preview.rows.is_empty());
        assert!(preview.errors.is_empty());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("应用模板.xlsx");
        std::fs::write(&path, b"old contents").unwrap();
        tauri::async_runtime::block_on(save_app_excel_template(path.clone())).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), TEMPLATE);

        let wrong_ext = dir.path().join("invalid.xls");
        assert!(
            tauri::async_runtime::block_on(save_app_excel_template(wrong_ext.clone())).is_err()
        );
        assert!(!wrong_ext.exists());
    }

    #[test]
    fn template_credential_columns_default_to_excel_text_format() {
        let mut archive = zip::ZipArchive::new(Cursor::new(TEMPLATE)).unwrap();
        let sheet = read_package_xml(&mut archive, "xl/worksheets/sheet1.xml").unwrap();
        let styles = read_package_xml(&mut archive, "xl/styles.xml").unwrap();
        let sheet = parse_package_xml(&sheet).unwrap();
        let styles = parse_package_xml(&styles).unwrap();
        let cell_styles = styles
            .root_element()
            .children()
            .find(|node| node.has_tag_name("cellXfs"))
            .unwrap();
        for index in ["5", "6"] {
            let column = sheet
                .descendants()
                .find(|node| {
                    node.has_tag_name("col")
                        && node.attribute("min") == Some(index)
                        && node.attribute("max") == Some(index)
                })
                .unwrap();
            let style = column.attribute("style").unwrap().parse::<usize>().unwrap();
            let style = cell_styles
                .children()
                .filter(|node| node.is_element())
                .nth(style)
                .unwrap();
            assert_eq!(style.attribute("numFmtId"), Some("49"));
            assert_eq!(style.attribute("applyNumberFormat"), Some("1"));
        }
    }

    #[test]
    fn template_preview_retains_credentials_internally_and_omits_them_from_json_and_debug() {
        let account = "000042";
        let password = "  test-secret\t\n ";
        let mut archive = zip::ZipArchive::new(Cursor::new(TEMPLATE)).unwrap();
        let sheet = read_package_xml(&mut archive, "xl/worksheets/sheet1.xml").unwrap();
        let rows = format!(
            r#"<x:row r="2">
                <x:c r="A2" t="str"><x:v>测试应用</x:v></x:c>
                <x:c r="B2" t="str"><x:v>https://example.test</x:v></x:c>
                <x:c r="E2" t="str"><x:v>{account}</x:v></x:c>
                <x:c r="F2" t="inlineStr"><x:is><x:t xml:space="preserve">{password}</x:t></x:is></x:c>
            </x:row>
            <x:row r="3">
                <x:c r="A3" t="str"><x:v>空密码</x:v></x:c>
                <x:c r="B3" t="str"><x:v>https://example.test/empty</x:v></x:c>
                <x:c r="E3" t="str"><x:v>{account}</x:v></x:c>
                <x:c r="F3"/>
            </x:row>
            <x:row r="4">
                <x:c r="A4" t="str"><x:v>仅填密码</x:v></x:c>
                <x:c r="B4" t="str"><x:v>https://example.test/spaces</x:v></x:c>
                <x:c r="F4" t="inlineStr"><x:is><x:t xml:space="preserve">  </x:t></x:is></x:c>
            </x:row>
            </x:sheetData>"#
        );
        let sheet = sheet.replace("</x:sheetData>", &rows);
        let bytes = rewrite_xlsx(("xl/worksheets/sheet1.xml", sheet.as_bytes()), None);
        let preview = tauri::async_runtime::block_on(preview_app_excel(
            "应用模板.xlsx".into(),
            STANDARD.encode(bytes),
        ))
        .unwrap();
        assert!(preview.errors.is_empty());
        assert_eq!(preview.rows.len(), 3);
        assert!(preview.rows[0].account == account);
        assert!(preview.rows[0].password == password);
        assert!(preview.rows[1].account == account);
        assert!(preview.rows[1].password.is_empty());
        assert!(preview.rows[2].account.is_empty());
        assert!(preview.rows[2].password == "  ");

        let json = serde_json::to_value(&preview).unwrap();
        assert_eq!(
            json["rows"][0],
            serde_json::json!({
                "row": 2,
                "name": "测试应用",
                "url": "https://example.test",
                "category": "",
                "description": ""
            }),
        );
        let json = json.to_string();
        let debug = format!("{preview:?}");
        for output in [json, debug] {
            for secret in ["account", "password", account, password.trim()] {
                assert!(!output.contains(secret), "凭据出现在预览输出中");
            }
        }
    }

    #[test]
    fn workbook_override_takes_precedence_over_xml_default() {
        let types = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
            <Default Extension="xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
            <Override PartName="/xl/workbook.xml" ContentType="application/vnd.ms-excel.sheet.macroEnabled.main+xml"/>
        </Types>"#;
        let macro_workbook = rewrite_xlsx(("[Content_Types].xml", types), None);
        assert_eq!(check_xlsx(&macro_workbook), Err(INVALID_FILE));
    }

    #[test]
    fn xlsx_uses_header_aliases_and_actual_rows_even_with_sparse_sheet() {
        let bytes = xlsx_with_sheet(
            r#"
              <row r="1"><c r="A1" t="inlineStr"><is><t> </t></is></c></row>
              <row r="3">
                <c r="A3" t="inlineStr"><is><t>说明</t></is></c>
                <c r="B3" t="inlineStr"><is><t>uRl</t></is></c>
                <c r="D3" t="inlineStr"><is><t>名称</t></is></c>
                <c r="Z3" t="inlineStr"><is><t>分类</t></is></c>
              </row>
              <row r="8">
                <c r="B8" t="str"><f>HYPERLINK("https://example.invalid")</f><v>https://example.test/Foo</v></c>
                <c r="D8" t="inlineStr"><is><t> 应用 </t></is></c>
              </row>
              <row r="1048576">
                <c r="D1048576" t="inlineStr"><is><t>名称</t></is></c>
                <c r="B1048576" t="inlineStr"><is><t>https://user:pass@example.test</t></is></c>
              </row>"#,
            None,
        );
        let preview = parse_excel(Path::new("fixture.xlsx"), &bytes).unwrap();
        assert_eq!(preview.rows.len(), 1);
        let row = &preview.rows[0];
        assert_eq!(row.row, 8);
        assert_eq!(row.name, "应用");
        assert_eq!(row.url, "https://example.test/Foo");
        assert!(row.category.is_empty());
        assert!(row.description.is_empty());
        assert!(row.account.is_empty());
        assert!(row.password.is_empty());
        assert_eq!(preview.errors.len(), 1);
        assert_eq!(preview.errors[0].row, 1_048_576);
        assert!(!preview.errors[0].message.contains("pass"));
    }

    fn record(workbook: &mut Vec<u8>, kind: u16, payload: &[u8]) {
        workbook.extend(kind.to_le_bytes());
        workbook.extend((payload.len() as u16).to_le_bytes());
        workbook.extend(payload);
    }

    fn xls_with_cells(cells: &[(u16, u16, &str)]) -> Vec<u8> {
        let mut workbook = Vec::new();
        record(&mut workbook, 0x0809, &[0, 6, 5, 0]);
        let start = (workbook.len() + 4 + 9 + 4) as u32;
        let mut bounds = start.to_le_bytes().to_vec();
        bounds.extend([0, 0, 1, 0, b'S']);
        record(&mut workbook, 0x0085, &bounds);
        record(&mut workbook, 0x000A, &[]);
        record(&mut workbook, 0x0809, &[0, 6, 0x10, 0]);
        for &(row, column, value) in cells {
            let mut label = Vec::new();
            label.extend(row.to_le_bytes());
            label.extend(column.to_le_bytes());
            label.extend([0, 0]);
            label.extend((value.encode_utf16().count() as u16).to_le_bytes());
            label.push(1);
            label.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
            record(&mut workbook, 0x0204, &label);
        }
        record(&mut workbook, 0x000A, &[]);
        let mut container = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        {
            let mut stream = container.create_stream("/Workbook").unwrap();
            stream.write_all(&workbook).unwrap();
            stream.flush().unwrap();
        }
        container.into_inner().into_inner()
    }

    #[test]
    fn xls_with_reordered_headers_and_real_row_offsets() {
        let xls = xls_with_cells(&[
            (4, 1, "描述"),
            (4, 2, "分类"),
            (4, 3, "连接地址"),
            (4, 4, "名称"),
            (9, 1, "旧版工作簿"),
            (9, 3, "https://example.test"),
            (9, 4, "应用"),
            (10, 3, "https://example.test/invalid"),
        ]);
        let preview = parse_excel(Path::new("fixture.xls"), &xls).unwrap();
        assert_eq!(preview.rows.len(), 1);
        assert_eq!(preview.rows[0].row, 10);
        assert_eq!(preview.rows[0].description, "旧版工作簿");
        assert!(preview.rows[0].account.is_empty());
        assert!(preview.rows[0].password.is_empty());
        assert_eq!(preview.errors.len(), 1);
        assert_eq!(preview.errors[0].row, 11);
        assert!(parse_excel(Path::new("wrong.xlsx"), &xls).is_err());
        assert!(parse_excel(Path::new("wrong.xls"), TEMPLATE).is_err());
    }

    #[test]
    fn preflight_rejects_expansion_and_oversized_xls_range() {
        let huge = vec![0; MAX_XLSX_BYTES as usize + 1];
        let xlsx = xlsx_with_sheet("", Some(("oversized.bin", &huge)));
        assert!(xlsx.len() < MAX_FILE_BYTES);
        assert_eq!(check_xlsx(&xlsx), Err(LARGE_SHEET));

        let xls = xls_with_cells(&[(0, 0, "开始"), (65_535, 255, "结束")]);
        assert_eq!(
            parse_excel(Path::new("fixture.xls"), &xls).unwrap_err(),
            LARGE_SHEET
        );
    }

    #[test]
    fn row_cap_counts_all_import_candidates() {
        let mut builder = PreviewBuilder::default();
        for (index, name) in COLUMN_NAMES.iter().enumerate() {
            builder
                .push_cell(0, index as u32, &Data::String((*name).to_owned()))
                .unwrap();
        }
        let name = Data::String("应用".into());
        for row in 1..=MAX_EXCEL_ROWS as u32 + 1 {
            builder.push_cell(row, 0, &name).unwrap();
        }
        assert_eq!(builder.result.errors.len(), MAX_EXCEL_ROWS);
        assert!(builder.finish().unwrap_err().contains("1000"));
    }
}
