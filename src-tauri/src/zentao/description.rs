//! 22.0 file::replaceImgURL 只展开双引号 src；保留其余原文逐字比较。
use super::*;

fn source_key(base: &str, source: &str) -> Option<String> {
    if let Some(inner) = source.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        let (id, ext) = inner.split_once('.')?;
        remote_id(&json!(id)).ok()?;
        return Some(format!("{id}.{ext}"));
    }
    let root = endpoint(base, "").ok()?;
    let url = root.join(&source.replace("&amp;", "&")).ok()?;
    if url.origin() != root.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let relative = url.path().strip_prefix(root.path())?;
    if let Some(name) = relative.strip_prefix("file-read-") {
        if url.query().is_some() {
            return None;
        }
        let (id, ext) = name.split_once('.')?;
        remote_id(&json!(id)).ok()?;
        return Some(format!("{id}.{ext}"));
    }
    if relative != "index.php" && !relative.is_empty() {
        return None;
    }
    let pairs: Vec<_> = url.query_pairs().collect();
    let mut values = HashMap::new();
    for (key, value) in pairs {
        if !["m", "f", "fileID", "t"].contains(&key.as_ref()) || values.insert(key, value).is_some()
        {
            return None;
        }
    }
    if values.get("m")?.as_ref() != "file" || values.get("f")?.as_ref() != "read" {
        return None;
    }
    let id = values.get("fileID")?;
    remote_id(&json!(id)).ok()?;
    Some(format!("{id}.{}", values.get("t")?))
}

fn normalized(base: &str, html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find(" src=\"") {
        let end_prefix = start + " src=\"".len();
        result.push_str(&rest[..end_prefix]);
        rest = &rest[end_prefix..];
        let Some(end) = rest.find('"') else {
            break;
        };
        let source = &rest[..end];
        if let Some(key) = source_key(base, source) {
            result.push('{');
            result.push_str(&key);
            result.push('}');
        } else {
            result.push_str(source);
        }
        result.push('"');
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    result
}

pub(super) fn field_matches(base: &str, field: &str, actual: &Value, expected: &Value) -> bool {
    if actual == expected {
        return true;
    }
    if field != "desc" {
        return false;
    }
    match (actual.as_str(), expected.as_str()) {
        (Some(actual), Some(expected)) => normalized(base, actual) == normalized(base, expected),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn description_accepts_only_same_site_image_expansion() {
        let base = "https://example.test/zentao";
        let raw = json!(r#"<p>原文</p><img src="{708.png}" width="240" />"#);
        for source in [
            "/zentao/file-read-708.png",
            "https://example.test/zentao/file-read-708.png",
            "index.php?m=file&amp;f=read&amp;fileID=708&amp;t=png",
        ] {
            let expanded = json!(format!(r#"<p>原文</p><img src="{source}" width="240" />"#));
            assert!(field_matches(base, "desc", &raw, &expanded));
            assert!(field_matches(base, "desc", &expanded, &raw));
        }
        for source in [
            "https://evil.test/zentao/file-read-708.png",
            "/other/file-read-708.png",
            "/zentao/file-read-709.png",
            "/zentao/file-read-708.jpg",
            "/zentao/file-read-708.png?download=1",
            "index.php?m=file&f=delete&fileID=708&t=png",
        ] {
            let changed = json!(format!(r#"<p>原文</p><img src="{source}" width="240" />"#));
            assert!(!field_matches(base, "desc", &raw, &changed), "{source}");
        }
        for changed in [
            r#"<p>新正文</p><img src="{708.png}" width="240" />"#,
            r#"<p>原文</p><img src="{708.png}" width="480" />"#,
            r#"<p>原文</p>"#,
        ] {
            assert!(!field_matches(base, "desc", &raw, &json!(changed)));
        }
        assert!(!field_matches(base, "name", &json!("a"), &json!("b")));
        assert!(!field_matches(base, "desc", &Value::Null, &json!("")));
    }
}
