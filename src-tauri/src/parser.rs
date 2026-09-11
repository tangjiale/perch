use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::OnceLock,
    time::Duration,
};
use tauri::Manager;
use tokio::{io::AsyncReadExt, process::Command, sync::Semaphore};

static PARSING: OnceLock<Semaphore> = OnceLock::new();
const OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;

pub fn resources(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/parser"))
    } else {
        Ok(app
            .path()
            .resource_dir()
            .map_err(|e| e.to_string())?
            .join("resources/parser"))
    }
}

pub async fn extract(root: &Path, path: &Path, extension: &str) -> Result<String, String> {
    if matches!(extension, "txt" | "md") {
        return tokio::fs::read_to_string(path)
            .await
            .map_err(|_| "文本必须是 UTF-8 编码".into());
    }
    let _permit = PARSING
        .get_or_init(|| Semaphore::new(1))
        .acquire()
        .await
        .map_err(|e| e.to_string())?;
    let java = if cfg!(target_os = "windows") {
        root.join("jre/bin/java.exe")
    } else {
        root.join("jre/Contents/Home/bin/java")
    };
    if !java.is_file() {
        return Err("随包文档解析运行时缺失，请重新安装应用".into());
    }
    let mut command = Command::new(java);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x08000000);
    let mut child = command
        .args(["-Xmx512m", "-Djava.awt.headless=true", "-jar"])
        .arg(root.join("tika-app.jar"))
        .arg(format!(
            "--config={}",
            root.join("tika-config.xml").display()
        ))
        .arg("--text")
        .arg(path)
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(if cfg!(test) {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "无法启动文档解析器")?;
    let stdout = child.stdout.take().ok_or("解析器输出不可用")?;
    let result = tokio::time::timeout(Duration::from_secs(60), async {
        let mut output = vec![];
        stdout
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut output)
            .await
            .map_err(|_| "读取解析结果失败")?;
        if output.len() as u64 > OUTPUT_LIMIT {
            return Err("文档正文超过 16 MB 限制".into());
        }
        let status = child.wait().await.map_err(|_| "文档解析进程失败")?;
        if !status.success() {
            return Err("文档解析失败，可能已损坏或受密码保护".into());
        }
        String::from_utf8(output).map_err(|_| "解析结果编码无效".into())
    })
    .await;
    match result {
        Ok(Ok(text)) => Ok(text.trim().to_owned()),
        Ok(Err(error)) => {
            let _ = child.kill().await;
            Err(error)
        }
        Err(_) => {
            let _ = child.kill().await;
            Err("文档解析超时（60 秒）".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn parser_root() -> PathBuf {
        std::env::var_os("WORKBENCH_PARSER_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/parser"))
    }
    #[tokio::test]
    async fn bundled_parser_reads_real_pptx() {
        let root = parser_root();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("presentation.pptx");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let files = [
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#,
            ),
            (
                "ppt/presentation.xml",
                r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="9144000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#,
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            (
                "ppt/slides/slide1.xml",
                r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2" name="Text"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Workbench PowerPoint fixture</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            ),
        ];
        for (name, content) in files {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            let content = if name == "[Content_Types].xml" {
                content.replace("</Types>","<Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/><Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/></Types>")
            } else {
                content.to_owned()
            };
            zip.write_all(content.as_bytes()).unwrap();
        }
        for (name, content) in [
            (
                "ppt/slides/_rels/slide1.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"#,
            ),
            (
                "ppt/slideLayouts/slideLayout1.xml",
                r#"<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld></p:sldLayout>"#,
            ),
            (
                "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#,
            ),
            (
                "ppt/slideMasters/slideMaster1.xml",
                r#"<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld></p:sldMaster>"#,
            ),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let result = extract(&root, &path, "pptx").await.unwrap();
        assert!(result.contains("Workbench PowerPoint fixture"), "{result}");
    }
    #[tokio::test]
    async fn bundled_parser_reads_real_xlsx() {
        let root = parser_root();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("workbook.xlsx");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let files = [
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Test" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Workbench Excel fixture</t></is></c></row></sheetData></worksheet>"#,
            ),
        ];
        for (name, content) in files {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let result = extract(&root, &path, "xlsx").await.unwrap();
        assert!(result.contains("Workbench Excel fixture"), "{result}");
    }
    #[tokio::test]
    async fn bundled_parser_reads_real_docx() {
        let root = parser_root();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("document.docx");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let files = [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            (
                "word/document.xml",
                r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Workbench knowledge fixture 中文解析</w:t></w:r></w:p></w:body></w:document>"#,
            ),
        ];
        for (name, content) in files {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let result = extract(&root, &path, "docx").await.unwrap();
        assert!(
            result.contains("Workbench knowledge fixture 中文解析"),
            "{result}"
        );
    }

    #[tokio::test]
    async fn bundled_parser_reads_real_pdf() {
        let root = parser_root();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("document.pdf");
        let stream = "BT /F1 12 Tf 50 750 Td (Workbench PDF fixture) Tj ET";
        let objects = ["<< /Type /Catalog /Pages 2 0 R >>".to_string(),"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string(),"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),format!("<< /Length {} >>\nstream\n{stream}\nendstream",stream.len())];
        let mut pdf = "%PDF-1.4\n".to_string();
        let mut offsets = vec![0];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
        }
        let xref = pdf.len();
        pdf.push_str("xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            pdf.push_str(&format!("{offset:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        ));
        std::fs::write(&path, pdf).unwrap();
        let result = extract(&root, &path, "pdf").await.unwrap();
        assert!(result.contains("Workbench PDF fixture"), "{result}");
    }
}
