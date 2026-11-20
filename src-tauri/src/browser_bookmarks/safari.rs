use super::{label, read_file, BookmarkNode, Issue, Tree, MAX_BOOKMARK_FILE};
use roxmltree::{Document, Node, ParsingOptions};
use std::path::Path;

const APPLE_DTD: &str = r#"<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">"#;

pub(super) fn read(path: &Path, tree: &mut Tree) -> Result<Vec<BookmarkNode>, Issue> {
    let mut bytes = read_file(path, MAX_BOOKMARK_FILE)?;
    if bytes.starts_with(b"bplist00") {
        bytes = convert_binary(&bytes)?;
    }
    let xml = std::str::from_utf8(&bytes).map_err(|_| Issue::Invalid)?;
    let document = xml.replacen(APPLE_DTD, "", 1);
    let document = Document::parse_with_options(
        &document,
        ParsingOptions {
            nodes_limit: 200_000,
            ..Default::default()
        },
    )
    .map_err(|_| Issue::Invalid)?;
    let root = document.root_element();
    let bookmarks = root
        .first_element_child()
        .filter(|node| root.has_tag_name("plist") && node.has_tag_name("dict"))
        .ok_or(Issue::Invalid)?;
    if value(bookmarks, "WebBookmarkFileVersion").is_none()
        && text(bookmarks, "WebBookmarkType") != Some("WebBookmarkTypeList")
    {
        return Err(Issue::Invalid);
    }
    read_children(bookmarks, tree, "root", None, 0)
}

fn value<'a, 'input>(dict: Node<'a, 'input>, key: &str) -> Option<Node<'a, 'input>> {
    let mut children = dict.children().filter(Node::is_element);
    while let Some(name) = children.next() {
        let value = children.next()?;
        if name.has_tag_name("key") && name.text() == Some(key) {
            return Some(value);
        }
    }
    None
}

fn text<'a>(dict: Node<'a, '_>, key: &str) -> Option<&'a str> {
    value(dict, key)
        .filter(|node| node.has_tag_name("string"))?
        .text()
}

fn read_children(
    dict: Node<'_, '_>,
    tree: &mut Tree,
    parent: &str,
    group: Option<&str>,
    depth: usize,
) -> Result<Vec<BookmarkNode>, Issue> {
    let Some(children) = value(dict, "Children") else {
        return Ok(Vec::new());
    };
    if !children.has_tag_name("array") {
        return Err(Issue::Invalid);
    }
    let mut nodes = Vec::new();
    for (index, child) in children.children().filter(Node::is_element).enumerate() {
        tree.visit(depth)?;
        if !child.has_tag_name("dict") {
            continue;
        }
        let title = text(child, "Title");
        let id = text(child, "WebBookmarkUUID");
        if title == Some("com.apple.ReadingList")
            || text(child, "WebBookmarkIdentifier") == Some("com.apple.ReadingList")
        {
            continue;
        }
        let node = match text(child, "WebBookmarkType") {
            Some("WebBookmarkTypeList") => {
                let id = tree.id(id, parent, index);
                let name = match (depth, title) {
                    (0, Some("BookmarksBar")) => "个人收藏".into(),
                    (0, Some("BookmarksMenu")) => "书签菜单".into(),
                    _ => label(title, "书签文件夹"),
                };
                let children = read_children(child, tree, &id, Some(&name), depth + 1)?;
                Some(BookmarkNode::Folder { id, name, children })
            }
            Some("WebBookmarkTypeLeaf") => {
                let name = value(child, "URIDictionary")
                    .and_then(|dict| text(dict, "title"))
                    .or(title);
                text(child, "URLString")
                    .and_then(|url| tree.bookmark(id, parent, index, name, url, group))
            }
            _ => None,
        };
        if let Some(node) = node {
            nodes.push(node);
        }
    }
    Ok(nodes)
}

#[cfg(target_os = "macos")]
fn convert_binary(bytes: &[u8]) -> Result<Vec<u8>, Issue> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut input = tempfile::NamedTempFile::new().map_err(|_| Issue::Io)?;
    input.write_all(bytes).map_err(|_| Issue::Io)?;
    let mut child = Command::new("/usr/bin/plutil")
        .args(["-convert", "xml1", "-o", "-", "--"])
        .arg(input.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Issue::Io)?;
    let stdout = child.stdout.take().expect("plutil 输出已建立管道");
    let limit = 2 * MAX_BOOKMARK_FILE;
    let output = std::thread::Builder::new().spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Issue::Io)?;
        if bytes.len() as u64 > limit {
            return Err(Issue::Limit);
        }
        Ok(bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = if output.is_ok() {
        loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => break None,
            }
        }
    } else {
        None
    };
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let bytes = output
        .map_err(|_| Issue::Io)?
        .join()
        .map_err(|_| Issue::Io)??;
    if status.is_some_and(|status| status.success()) {
        Ok(bytes)
    } else {
        Err(Issue::Invalid)
    }
}

#[cfg(not(target_os = "macos"))]
fn convert_binary(_bytes: &[u8]) -> Result<Vec<u8>, Issue> {
    Err(Issue::Unsupported)
}
