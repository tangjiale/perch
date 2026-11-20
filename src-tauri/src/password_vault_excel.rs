use crate::{
    app_excel::{
        cell_text, read_excel_file, save_excel_template, validate_cell_text, visit_excel_cells,
        AppExcelError, INVALID_SHEET, LARGE_SHEET, MAX_EXCEL_ROWS, MAX_PREVIEW_BYTES,
    },
    password_vault::{VaultEnvironment, VaultFields, VaultKind},
};
use calamine::DataType;
use serde::Serialize;
use std::path::{Path, PathBuf};

const APPLICATION_COLUMNS: [&str; 7] = [
    "应用名称",
    "应用编号",
    "地址",
    "账号1",
    "账号2",
    "密码",
    "备注",
];
const SERVER_COLUMNS: [&str; 5] = ["IP", "账号", "密码", "环境", "备注"];
const APPLICATION_TEMPLATE: &[u8] =
    include_bytes!("../../public/templates/password-vault-applications.xlsx");
const SERVER_TEMPLATE: &[u8] = include_bytes!("../../public/templates/password-vault-servers.xlsx");

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultExcelRow {
    pub row: usize,
    #[serde(flatten)]
    pub fields: VaultFields,
    pub has_password: bool,
    #[serde(skip_serializing)]
    pub password: String,
}

#[derive(Default, Serialize)]
pub struct VaultExcelPreview {
    pub rows: Vec<VaultExcelRow>,
    pub errors: Vec<AppExcelError>,
}

#[tauri::command]
pub async fn preview_password_vault_excel(
    kind: VaultKind,
    file_name: String,
    contents: String,
) -> Result<VaultExcelPreview, String> {
    read_password_vault_excel(kind, file_name, contents).await
}

pub(crate) async fn read_password_vault_excel(
    kind: VaultKind,
    file_name: String,
    contents: String,
) -> Result<VaultExcelPreview, String> {
    read_excel_file(file_name, contents, move |path, bytes| {
        parse_excel(kind, path, bytes)
    })
    .await
}

#[tauri::command]
pub async fn save_password_vault_template(kind: VaultKind, path: PathBuf) -> Result<(), String> {
    let template = match kind {
        VaultKind::Application => APPLICATION_TEMPLATE,
        VaultKind::Server => SERVER_TEMPLATE,
    };
    save_excel_template(path, template).await
}

fn parse_excel(
    kind: VaultKind,
    path: &Path,
    bytes: &[u8],
) -> Result<VaultExcelPreview, &'static str> {
    let mut preview = PreviewBuilder::new(kind);
    visit_excel_cells(path, bytes, |row, column, value| {
        preview.push_cell(row, column, value)
    })?;
    preview.finish()
}

struct PreviewBuilder {
    kind: VaultKind,
    columns: &'static [&'static str],
    result: VaultExcelPreview,
    header: [Option<u32>; APPLICATION_COLUMNS.len()],
    header_ready: bool,
    row: Option<u32>,
    values: [String; APPLICATION_COLUMNS.len()],
    cell_error: Option<&'static str>,
    retained_bytes: usize,
}

impl PreviewBuilder {
    fn new(kind: VaultKind) -> Self {
        Self {
            kind,
            columns: match kind {
                VaultKind::Application => &APPLICATION_COLUMNS,
                VaultKind::Server => &SERVER_COLUMNS,
            },
            result: VaultExcelPreview::default(),
            header: [None; APPLICATION_COLUMNS.len()],
            header_ready: false,
            row: None,
            values: Default::default(),
            cell_error: None,
            retained_bytes: 0,
        }
    }

    fn push_cell(
        &mut self,
        row: u32,
        column: u32,
        value: &dyn DataType,
    ) -> Result<(), &'static str> {
        if value.is_empty() {
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

        if self.header_ready {
            self.row = Some(row);
            let Some(index) = self.header.iter().position(|entry| *entry == Some(column)) else {
                return Ok(());
            };
            let text = cell_text(value);
            let text = if self.columns[index] == "密码" {
                text.as_ref()
            } else {
                text.trim()
            };
            match validate_cell_text(value, text) {
                Ok(()) => self.values[index] = text.to_owned(),
                Err(message) => self.cell_error = Some(message),
            }
        } else {
            let text = cell_text(value);
            let title = text.trim();
            if title.is_empty() {
                return Ok(());
            }
            self.row = Some(row);
            // 旧版应用模板把第一个账号列写成「账号」。
            let title = match (self.kind, title) {
                (VaultKind::Application, "账号") => "账号1",
                _ => title,
            };
            if let Some(index) = self.columns.iter().position(|entry| *entry == title) {
                if self.header.contains(&Some(column))
                    || self.header[index].replace(column).is_some()
                {
                    return Err("表头中有重复的导入列，请检查并保留一个标题。");
                }
            }
        }
        Ok(())
    }

    fn complete_row(&mut self, row: u32) -> Result<(), &'static str> {
        if !self.header_ready {
            match (self.kind, self.header[0], self.header[3]) {
                (VaultKind::Application, None, _) => return Err("首个非空行缺少应用名称表头。"),
                (VaultKind::Server, None, None) => return Err("首个非空行缺少 IP 和环境表头。"),
                (VaultKind::Server, None, _) => return Err("首个非空行缺少 IP 表头。"),
                (VaultKind::Server, _, None) => return Err("首个非空行缺少环境表头。"),
                _ => {}
            }
            self.header_ready = true;
            return Ok(());
        }

        let values = std::mem::take(&mut self.values);
        let cell_error = self.cell_error.take();
        let bytes = values.iter().map(String::len).sum::<usize>();
        if cell_error.is_none() && bytes == 0 {
            return Ok(());
        }
        if self.result.rows.len() + self.result.errors.len() >= MAX_EXCEL_ROWS {
            return Err("每次最多预览 1000 条记录，请拆分 Excel 文件后重试。");
        }

        let row = row as usize + 1;
        let result = match cell_error {
            Some(message) => Err(message.into()),
            None => validate_row(self.kind, values),
        };
        match result {
            Ok((fields, password)) => {
                self.retained_bytes += bytes;
                if self.retained_bytes > MAX_PREVIEW_BYTES {
                    return Err(LARGE_SHEET);
                }
                self.result.rows.push(VaultExcelRow {
                    row,
                    fields,
                    has_password: !password.is_empty(),
                    password,
                });
            }
            Err(message) => self.result.errors.push(AppExcelError { row, message }),
        }
        Ok(())
    }

    fn finish(mut self) -> Result<VaultExcelPreview, &'static str> {
        let row = self.row.ok_or("首张工作表为空，请添加表头。")?;
        self.complete_row(row)?;
        Ok(self.result)
    }
}

fn validate_row(
    kind: VaultKind,
    values: [String; APPLICATION_COLUMNS.len()],
) -> Result<(VaultFields, String), String> {
    let (fields, password) = match kind {
        VaultKind::Application => {
            let [app_name, app_code, address, account1, account2, password, notes] = values;
            (
                VaultFields::Application {
                    app_name,
                    app_code,
                    address,
                    account1,
                    account2,
                    notes,
                },
                password,
            )
        }
        VaultKind::Server => {
            let [ip, account, password, environment, notes, ..] = values;
            if ip.is_empty() {
                return Err("缺少 IP。".into());
            }
            let environment = match environment.as_str() {
                "dev" => VaultEnvironment::Dev,
                "test" => VaultEnvironment::Test,
                "prod" => VaultEnvironment::Prod,
                "" => return Err("缺少环境。".into()),
                _ => return Err("环境只能填写 dev、test 或 prod。".into()),
            };
            (
                VaultFields::Server {
                    ip,
                    account,
                    environment,
                    notes,
                },
                password,
            )
        }
    };
    Ok((fields.validate()?, password))
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::Data;
    use serde_json::json;

    fn preview_cells(kind: VaultKind, cells: &[(u32, u32, &str)]) -> VaultExcelPreview {
        let mut preview = PreviewBuilder::new(kind);
        for &(row, column, value) in cells {
            preview
                .push_cell(row, column, &Data::String(value.into()))
                .unwrap();
        }
        preview.finish().unwrap()
    }

    #[test]
    fn embedded_templates_parse_as_empty_previews() {
        for (kind, template) in [
            (VaultKind::Application, APPLICATION_TEMPLATE),
            (VaultKind::Server, SERVER_TEMPLATE),
        ] {
            let preview = parse_excel(kind, Path::new("模板.XLSX"), template).unwrap();
            assert!(preview.rows.is_empty());
            assert!(preview.errors.is_empty());
        }
    }

    #[test]
    fn legacy_application_columns_and_password_whitespace() {
        let preview = preview_cells(
            VaultKind::Application,
            &[
                (3, 0, "密码"),
                (3, 2, "账号"),
                (3, 4, "应用名称"),
                (3, 5, "环境"),
                (3, 6, "地址"),
                (7, 0, " \t "),
                (9, 0, "  fixture  "),
                (9, 2, "00012"),
                (9, 4, "门户"),
                (9, 5, "ignored"),
                (9, 6, "内网手册"),
            ],
        );
        assert_eq!(preview.errors.len(), 1);
        assert_eq!(preview.errors[0].row, 8);
        assert_eq!(preview.rows.len(), 1);
        let row = &preview.rows[0];
        assert!(row.password == "  fixture  ");
        assert_eq!(
            serde_json::to_value(row).unwrap(),
            json!({
                "row": 10, "kind": "application", "appName": "门户", "appCode": "",
                "address": "内网手册", "account1": "00012", "account2": "", "notes": "",
                "hasPassword": true
            })
        );

        let preview = preview_cells(
            VaultKind::Application,
            &[(0, 0, "应用名称"), (1, 0, "仅应用名称")],
        );
        assert_eq!(preview.rows.len(), 1);
        assert!(!preview.rows[0].has_password);
    }

    #[test]
    fn server_validation_reports_precise_sheet_rows() {
        let preview = preview_cells(
            VaultKind::Server,
            &[
                (0, 0, "环境"),
                (0, 1, "IP"),
                (1, 0, "test"),
                (1, 1, "192.0.2.1"),
                (5, 0, "prod"),
                (5, 1, "2001:db8::1"),
                (6, 0, "PROD"),
                (6, 1, "192.0.2.3"),
                (12, 1, "192.0.2.4"),
                (20, 0, "dev"),
                (20, 1, "example.invalid"),
            ],
        );
        assert_eq!(preview.rows.len(), 2);
        assert_eq!(preview.rows[0].row, 2);
        assert_eq!(preview.rows[1].row, 6);
        assert_eq!(preview.errors.len(), 3);
        assert_eq!(preview.errors[0].row, 7);
        assert_eq!(
            preview.errors[0].message,
            "环境只能填写 dev、test 或 prod。"
        );
        assert_eq!(preview.errors[1].row, 13);
        assert_eq!(preview.errors[1].message, "缺少环境。");
        assert_eq!(preview.errors[2].row, 21);
        assert!(preview.errors[2].message.contains("IPv4 或 IPv6"));
    }
}
