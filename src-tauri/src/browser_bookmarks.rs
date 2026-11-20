use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

mod chromium;
mod firefox;
mod safari;

const MAX_PROFILES: usize = 48;
const MAX_DIRECTORY_ENTRIES: usize = 1024;
const MAX_PROFILE_NODES: usize = 20_000;
const MAX_TOTAL_NODES: usize = 80_000;
const MAX_DEPTH: usize = 48;
const MAX_URL: usize = 16_384;
const MAX_BOOKMARK_FILE: u64 = 16 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct BrowserBookmarks {
    id: &'static str,
    name: &'static str,
    status: BrowserStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    profiles: Vec<BookmarkProfile>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum BrowserStatus {
    Ready,
    Unavailable,
    Error,
}

#[derive(Debug, Serialize)]
struct BookmarkProfile {
    id: String,
    name: String,
    nodes: Vec<BookmarkNode>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum BookmarkNode {
    Folder {
        id: String,
        name: String,
        children: Vec<BookmarkNode>,
    },
    Bookmark {
        id: String,
        name: String,
        url: String,
        group: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Issue {
    Missing,
    Permission,
    Invalid,
    Limit,
    Busy,
    Io,
    Unsupported,
}

impl Issue {
    fn message(self) -> &'static str {
        match self {
            Self::Missing => "标准目录中没有找到书签文件。",
            Self::Permission => "没有读取书签文件的权限，请检查系统隐私和文件访问设置。",
            Self::Invalid => "书签文件或配置格式无效，可能已损坏。",
            Self::Limit => "书签文件、目录或条目数量超出扫描上限。",
            Self::Busy => "书签文件正在变化，请稍后重试或关闭浏览器后再扫描。",
            Self::Io => "无法读取书签文件，请稍后重试。",
            Self::Unsupported => "此系统没有受支持的 Safari 版本。",
        }
    }
}

impl From<io::Error> for Issue {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::Missing,
            io::ErrorKind::PermissionDenied => Self::Permission,
            io::ErrorKind::WouldBlock => Self::Busy,
            _ => Self::Io,
        }
    }
}

fn is_local(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    !matches!(path.components().next(), Some(Component::Prefix(prefix))
        if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
}

fn regular_metadata(path: &Path) -> Result<fs::Metadata, Issue> {
    if !is_local(path) {
        return Err(Issue::Permission);
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(Issue::Invalid);
    }
    Ok(metadata)
}

fn open_file(path: &Path) -> Result<File, Issue> {
    regular_metadata(path)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Issue::Invalid);
    }
    Ok(file)
}

fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>, Issue> {
    if regular_metadata(path)?.len() > limit {
        return Err(Issue::Limit);
    }
    let mut bytes = Vec::new();
    open_file(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(Issue::Limit);
    }
    Ok(bytes)
}

fn child_dirs(root: &Path) -> Result<Vec<PathBuf>, Issue> {
    if !is_local(root) {
        return Err(Issue::Permission);
    }
    let mut dirs = Vec::new();
    for (index, entry) in fs::read_dir(root)?.enumerate() {
        if index == MAX_DIRECTORY_ENTRIES {
            return Err(Issue::Limit);
        }
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn stable_id(kind: &str, parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("{kind}-{}", hex::encode(&hash.finalize()[..16]))
}

fn label(value: Option<&str>, fallback: &str) -> String {
    value
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(fallback)
        .chars()
        .filter(|ch| !ch.is_control())
        .take(512)
        .collect()
}

fn folder_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("浏览配置")
}

fn allowed_url(raw: &str) -> Option<&str> {
    let (scheme, rest) = raw.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || raw.len() > MAX_URL
        || host.is_empty()
        || host.contains('@')
        || raw.contains('\\')
        || raw.chars().any(|ch| ch.is_control() || ch.is_whitespace())
        || !url::Url::parse(raw).is_ok_and(|url| url.has_host())
    {
        return None;
    }
    Some(raw)
}

struct Tree {
    profile: String,
    seen: HashSet<String>,
    remaining: usize,
}

impl Tree {
    fn visit(&mut self, depth: usize) -> Result<(), Issue> {
        if depth > MAX_DEPTH || self.remaining == 0 {
            return Err(Issue::Limit);
        }
        self.remaining -= 1;
        Ok(())
    }

    fn id(&mut self, native: Option<&str>, parent: &str, index: usize) -> String {
        if let Some(native) = native.filter(|id| !id.is_empty() && id.len() <= 128) {
            let id = stable_id(
                "node",
                &[self.profile.as_bytes(), b"native", native.as_bytes()],
            );
            if self.seen.insert(id.clone()) {
                return id;
            }
        }
        stable_id(
            "node",
            &[
                self.profile.as_bytes(),
                b"position",
                parent.as_bytes(),
                &index.to_be_bytes(),
            ],
        )
    }

    fn bookmark(
        &mut self,
        native: Option<&str>,
        parent: &str,
        index: usize,
        name: Option<&str>,
        url: &str,
        group: Option<&str>,
    ) -> Option<BookmarkNode> {
        let url = allowed_url(url.trim())?;
        Some(BookmarkNode::Bookmark {
            id: self.id(native, parent, index),
            name: label(name, url),
            url: url.into(),
            group: group.map(str::to_owned),
        })
    }
}

#[derive(Clone, Copy)]
enum Reader {
    Chromium,
    Firefox,
    Safari,
}

struct Source {
    id: &'static str,
    name: &'static str,
    reader: Reader,
    roots: Vec<PathBuf>,
}

#[derive(Clone, Copy)]
enum Layout {
    Mac,
    Windows,
    Unix,
}

fn sources(
    layout: Layout,
    home: Option<&Path>,
    config: Option<&Path>,
    local: Option<&Path>,
) -> Vec<Source> {
    use Layout::*;
    let mut browsers = Vec::new();
    for (id, name, mac, windows, unix) in [
        (
            "chrome",
            "Chrome",
            &[
                "Google/Chrome",
                "Google/Chrome Beta",
                "Google/Chrome Dev",
                "Google/Chrome Canary",
            ][..],
            &[
                "Google/Chrome/User Data",
                "Google/Chrome Beta/User Data",
                "Google/Chrome Dev/User Data",
                "Google/Chrome SxS/User Data",
            ][..],
            &[
                "google-chrome",
                "google-chrome-beta",
                "google-chrome-unstable",
            ][..],
        ),
        (
            "edge",
            "Edge",
            &[
                "Microsoft Edge",
                "Microsoft Edge Beta",
                "Microsoft Edge Dev",
                "Microsoft Edge Canary",
            ][..],
            &[
                "Microsoft/Edge/User Data",
                "Microsoft/Edge Beta/User Data",
                "Microsoft/Edge Dev/User Data",
                "Microsoft/Edge SxS/User Data",
            ][..],
            &[
                "microsoft-edge",
                "microsoft-edge-beta",
                "microsoft-edge-dev",
            ][..],
        ),
    ] {
        let (base, suffixes) = match layout {
            Mac => (config, mac),
            Windows => (local, windows),
            Unix => (config, unix),
        };
        let roots = suffixes
            .iter()
            .filter_map(|suffix| base.map(|path| path.join(suffix)))
            .collect();
        browsers.push(Source {
            id,
            name,
            reader: Reader::Chromium,
            roots,
        });
    }
    let firefox = match layout {
        Mac => config
            .map(|path| path.join("Firefox"))
            .into_iter()
            .collect(),
        Windows => config
            .map(|path| path.join("Mozilla/Firefox"))
            .into_iter()
            .chain(
                [
                    "Mozilla.Firefox_n80bbvh6b1yt2",
                    "Mozilla.MozillaFirefox_gmpnhwe7bv608",
                ]
                .into_iter()
                .filter_map(|package| {
                    local.map(|path| {
                        path.join("Packages")
                            .join(package)
                            .join("LocalCache/Roaming/Mozilla/Firefox")
                    })
                }),
            )
            .collect(),
        Unix => [
            ".mozilla/firefox",
            "snap/firefox/common/.mozilla/firefox",
            ".var/app/org.mozilla.firefox/.mozilla/firefox",
        ]
        .into_iter()
        .filter_map(|suffix| home.map(|path| path.join(suffix)))
        .collect(),
    };
    browsers.push(Source {
        id: "firefox",
        name: "Firefox",
        reader: Reader::Firefox,
        roots: firefox,
    });
    let roots = if matches!(layout, Mac) {
        home.map(|path| path.join("Library/Safari/Bookmarks.plist"))
            .into_iter()
            .collect()
    } else {
        Vec::new()
    };
    browsers.push(Source {
        id: "safari",
        name: "Safari",
        reader: Reader::Safari,
        roots,
    });
    browsers
}

struct Scan {
    id: &'static str,
    profiles: Vec<BookmarkProfile>,
    issue: Option<Issue>,
    attempted: HashSet<PathBuf>,
    firefox_budget: u64,
}

impl Scan {
    fn note(&mut self, issue: Issue) {
        if issue != Issue::Missing {
            self.issue.get_or_insert(issue);
        }
    }

    fn profile(&mut self, path: &Path, name: String, reader: Reader, remaining: &mut usize) {
        match regular_metadata(path) {
            Ok(_) => {}
            Err(issue) => {
                self.note(issue);
                return;
            }
        }
        if self.attempted.len() == MAX_PROFILES || *remaining == 0 {
            self.note(Issue::Limit);
            return;
        }
        let canonical = match fs::canonicalize(path) {
            Ok(path) if is_local(&path) => path,
            Ok(_) => {
                self.note(Issue::Permission);
                return;
            }
            Err(error) => {
                self.note(error.into());
                return;
            }
        };
        if !self.attempted.insert(canonical.clone()) {
            return;
        }
        let id = stable_id(
            "profile",
            &[self.id.as_bytes(), canonical.as_os_str().as_encoded_bytes()],
        );
        let limit = (*remaining).min(MAX_PROFILE_NODES);
        let mut tree = Tree {
            profile: id.clone(),
            seen: HashSet::new(),
            remaining: limit,
        };
        let nodes = match reader {
            Reader::Chromium => chromium::read(path, &mut tree),
            Reader::Firefox => firefox::read(path, &mut tree, &mut self.firefox_budget),
            Reader::Safari => safari::read(path, &mut tree),
        };
        match nodes {
            Ok(nodes) => {
                *remaining -= limit - tree.remaining;
                self.profiles.push(BookmarkProfile { id, name, nodes });
            }
            Err(issue) => self.note(issue),
        }
    }
}

fn scan_browser(source: Source, remaining: &mut usize) -> BrowserBookmarks {
    let mut scan = Scan {
        id: source.id,
        profiles: Vec::new(),
        issue: None,
        attempted: HashSet::new(),
        firefox_budget: firefox::MAX_SCAN_BYTES,
    };
    if source.roots.is_empty()
        && matches!(source.reader, Reader::Safari)
        && !cfg!(target_os = "macos")
    {
        scan.note(Issue::Unsupported);
    }
    for (root_index, root) in source.roots.iter().enumerate() {
        match source.reader {
            Reader::Chromium => chromium::scan(root, root_index, &mut scan, remaining),
            Reader::Firefox => firefox::scan(root, &mut scan, remaining),
            Reader::Safari => scan.profile(root, "共享书签".into(), source.reader, remaining),
        }
    }
    let status = match (scan.profiles.is_empty(), scan.issue) {
        (false, _) => BrowserStatus::Ready,
        (true, None | Some(Issue::Unsupported)) => BrowserStatus::Unavailable,
        (true, Some(_)) => BrowserStatus::Error,
    };
    let message = match (&status, scan.issue) {
        (BrowserStatus::Ready, None) => None,
        (BrowserStatus::Ready, Some(issue)) => {
            Some(format!("部分浏览配置未能读取。{}", issue.message()))
        }
        (_, issue) => Some(issue.unwrap_or(Issue::Missing).message().into()),
    };
    BrowserBookmarks {
        id: source.id,
        name: source.name,
        status,
        message,
        profiles: scan.profiles,
    }
}

#[tauri::command]
pub async fn read_browser_bookmarks() -> Result<Vec<BrowserBookmarks>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let layout = if cfg!(target_os = "macos") {
            Layout::Mac
        } else if cfg!(windows) {
            Layout::Windows
        } else {
            Layout::Unix
        };
        let mut remaining = MAX_TOTAL_NODES;
        sources(
            layout,
            dirs::home_dir().as_deref(),
            dirs::config_dir().as_deref(),
            dirs::data_local_dir().as_deref(),
        )
        .into_iter()
        .map(|source| scan_browser(source, &mut remaining))
        .collect()
    })
    .await
    .map_err(|_| "浏览器书签扫描未能完成，请重试。".into())
}

#[cfg(test)]
mod tests;
