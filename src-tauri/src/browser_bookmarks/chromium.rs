use super::{
    child_dirs, folder_name, label, read_file, BookmarkNode, Issue, Reader, Scan, Tree,
    MAX_BOOKMARK_FILE,
};
use serde_json::Value;
use std::path::Path;

pub(super) fn scan(root: &Path, root_index: usize, scan: &mut Scan, remaining: &mut usize) {
    let dirs = match child_dirs(root) {
        Ok(dirs) => dirs,
        Err(issue) => {
            scan.note(issue);
            return;
        }
    };
    for directory in dirs {
        let name = folder_name(&directory);
        if name == "Guest Profile" || name == "System Profile" {
            continue;
        }
        let mut name = label(Some(name), "浏览配置");
        if root_index != 0 {
            let channel = if folder_name(root) == "User Data" {
                root.parent().unwrap_or(root)
            } else {
                root
            };
            name.push_str(" · ");
            name.push_str(folder_name(channel));
        }
        scan.profile(
            &directory.join("Bookmarks"),
            name.clone(),
            Reader::Chromium,
            remaining,
        );
        scan.profile(
            &directory.join("AccountBookmarks"),
            format!("{name} · 账号书签"),
            Reader::Chromium,
            remaining,
        );
    }
}

pub(super) fn read(path: &Path, tree: &mut Tree) -> Result<Vec<BookmarkNode>, Issue> {
    let bytes = read_file(path, MAX_BOOKMARK_FILE)?;
    let document: Value = serde_json::from_slice(&bytes).map_err(|_| Issue::Invalid)?;
    let roots = document["roots"]
        .as_object()
        .filter(|roots| !roots.is_empty())
        .ok_or(Issue::Invalid)?;
    let built_in = [
        ("bookmark_bar", "书签栏"),
        ("other", "其他书签"),
        ("synced", "移动设备书签"),
    ];
    let top = built_in
        .iter()
        .filter_map(|(key, fallback)| roots.get(*key).map(|value| (value, *fallback)))
        .chain(
            roots
                .iter()
                .filter(|(key, _)| !built_in.iter().any(|(known, _)| key == known))
                .map(|(_, value)| (value, "书签文件夹")),
        );
    let mut nodes = Vec::new();
    for (index, (value, fallback)) in top.enumerate() {
        if let Some(node) = read_node(value, tree, "root", index, None, fallback, 0)? {
            nodes.push(node);
        }
    }
    if nodes.is_empty() && !roots.values().any(|node| node["type"] == "folder") {
        return Err(Issue::Invalid);
    }
    Ok(nodes)
}

fn read_node(
    value: &Value,
    tree: &mut Tree,
    parent: &str,
    index: usize,
    group: Option<&str>,
    fallback: &str,
    depth: usize,
) -> Result<Option<BookmarkNode>, Issue> {
    tree.visit(depth)?;
    let native = value["guid"].as_str().or_else(|| value["id"].as_str());
    let name = value["name"].as_str();
    if value["type"] == "url" {
        return Ok(value["url"]
            .as_str()
            .and_then(|url| tree.bookmark(native, parent, index, name, url, group)));
    }
    if value["type"] != "folder" {
        return Ok(None);
    }
    let id = tree.id(native, parent, index);
    let name = label(name, fallback);
    let mut children = Vec::new();
    if let Some(entries) = value["children"].as_array() {
        for (index, child) in entries.iter().enumerate() {
            if let Some(node) = read_node(
                child,
                tree,
                &id,
                index,
                Some(&name),
                "书签文件夹",
                depth + 1,
            )? {
                children.push(node);
            }
        }
    }
    Ok(Some(BookmarkNode::Folder { id, name, children }))
}
