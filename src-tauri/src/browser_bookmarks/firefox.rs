use super::{
    child_dirs, folder_name, is_local, label, open_file, read_file, regular_metadata, BookmarkNode,
    Issue, Reader, Scan, Tree, MAX_URL,
};
use rusqlite::{Connection, OpenFlags};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

const MAX_INI: u64 = 256 * 1024;
const MAX_DB: u64 = 384 * 1024 * 1024;
const MAX_WAL: u64 = 128 * 1024 * 1024;
pub(super) const MAX_SCAN_BYTES: u64 = 1024 * 1024 * 1024;

pub(super) fn scan(root: &Path, scan: &mut Scan, remaining: &mut usize) {
    let mut profiles = BTreeMap::new();
    match read_file(&root.join("profiles.ini"), MAX_INI) {
        Ok(bytes) => match std::str::from_utf8(&bytes) {
            Ok(ini) => from_ini(ini, root, &mut profiles, scan),
            Err(_) => scan.note(Issue::Invalid),
        },
        Err(issue) => scan.note(issue),
    }
    // 新建配置和 Linux 的默认配置也可能只出现在目录中。
    for directory in [root.join("Profiles"), root.to_path_buf()] {
        match child_dirs(&directory) {
            Ok(dirs) => {
                for dir in dirs {
                    profiles
                        .entry(dir.clone())
                        .or_insert_with(|| label(Some(folder_name(&dir)), "Firefox 配置"));
                }
            }
            Err(issue) => scan.note(issue),
        }
    }
    for (profile, name) in profiles {
        scan.profile(
            &profile.join("places.sqlite"),
            name,
            Reader::Firefox,
            remaining,
        );
    }
}

fn from_ini(ini: &str, root: &Path, profiles: &mut BTreeMap<PathBuf, String>, scan: &mut Scan) {
    let mut in_profile = false;
    let mut path = None;
    let mut relative = None;
    let mut name = None;
    for line in ini
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .chain(std::iter::once("[End]"))
    {
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            if in_profile {
                if let Some(directory) = path
                    .take()
                    .and_then(|path| profile_path(root, path, relative.take()))
                {
                    let name = label(name.take(), folder_name(&directory));
                    profiles.insert(directory, name);
                } else {
                    scan.note(Issue::Invalid);
                }
            }
            path = None;
            relative = None;
            name = None;
            in_profile = section.strip_prefix("Profile").is_some_and(|index| {
                !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit())
            });
        } else if in_profile {
            match line.split_once('=') {
                Some(("Path", value)) => path = Some(value),
                Some(("IsRelative", value)) => relative = Some(value),
                Some(("Name", value)) => name = Some(value),
                _ => {}
            }
        }
    }
}

fn profile_path(root: &Path, value: &str, relative: Option<&str>) -> Option<PathBuf> {
    let path = Path::new(value);
    match relative {
        Some("0") if path.is_absolute() && is_local(path) => Some(path.to_path_buf()),
        Some("1")
            if !value.is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))) =>
        {
            Some(root.join(path))
        }
        _ => None,
    }
}

#[derive(PartialEq)]
struct Stamp {
    len: u64,
    modified: SystemTime,
    header: Vec<u8>,
    #[cfg(unix)]
    identity: (u64, u64),
}

fn stamp(path: &Path, size_limit: u64) -> Result<Option<Stamp>, Issue> {
    let file = match open_file(path) {
        Ok(file) => file,
        Err(Issue::Missing) => return Ok(None),
        Err(issue) => return Err(issue),
    };
    let metadata = file.metadata()?;
    let len = metadata.len();
    if len > size_limit {
        return Err(Issue::Limit);
    }
    let mut header = Vec::new();
    file.take(100).read_to_end(&mut header)?;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(Some(Stamp {
        len,
        modified: metadata.modified()?,
        header,
        #[cfg(unix)]
        identity: (metadata.dev(), metadata.ino()),
    }))
}

fn sidecar(db: &Path, suffix: &str) -> PathBuf {
    let mut path = db.as_os_str().to_owned();
    path.push(suffix);
    path.into()
}

fn active_rollback_journal(db: &Path) -> Result<bool, Issue> {
    match regular_metadata(&sidecar(db, "-journal")) {
        Ok(metadata) => Ok(metadata.len() != 0),
        Err(Issue::Missing) => Ok(false),
        Err(issue) => Err(issue),
    }
}

fn copy_file(source: &Path, dest: &Path, len: u64) -> Result<(), Issue> {
    let mut input = open_file(source)?.take(len + 1);
    if io::copy(&mut input, &mut File::create(dest)?)? != len {
        return Err(Issue::Busy);
    }
    Ok(())
}

fn snapshot(db: &Path, dest: &Path, budget: &mut u64) -> Result<(), Issue> {
    let wal = sidecar(db, "-wal");
    let dest_wal = sidecar(dest, "-wal");
    if active_rollback_journal(db)? {
        return Err(Issue::Busy);
    }
    let main = stamp(db, MAX_DB)?.ok_or(Issue::Missing)?;
    let wal_before = stamp(&wal, MAX_WAL)?;
    *budget = budget
        .checked_sub(main.len + wal_before.as_ref().map_or(0, |stamp| stamp.len))
        .ok_or(Issue::Limit)?;
    copy_file(db, dest, main.len)?;
    if let Some(meta) = &wal_before {
        copy_file(&wal, &dest_wal, meta.len).map_err(|issue| {
            if issue == Issue::Missing {
                Issue::Busy
            } else {
                issue
            }
        })?;
    }
    if stamp(db, MAX_DB)?.as_ref() != Some(&main)
        || stamp(&wal, MAX_WAL)? != wal_before
        || active_rollback_journal(db)?
    {
        return Err(Issue::Busy);
    }
    Ok(())
}

pub(super) fn read(
    path: &Path,
    tree: &mut Tree,
    budget: &mut u64,
) -> Result<Vec<BookmarkNode>, Issue> {
    // SQLite 只打开私有快照，浏览器目录不会产生 SHM、锁文件或 checkpoint。
    let mut temp = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.permissions(std::fs::Permissions::from_mode(0o700));
    }
    for attempt in 0..3 {
        let tempdir = temp.tempdir().map_err(|_| Issue::Io)?;
        let db = tempdir.path().join("places.sqlite");
        match snapshot(path, &db, budget) {
            Ok(()) => return read_snapshot(&db, tree),
            Err(Issue::Busy) if attempt < 2 => {
                std::thread::sleep(std::time::Duration::from_millis(35))
            }
            Err(issue) => return Err(issue),
        }
    }
    Err(Issue::Busy)
}

struct Row {
    id: i64,
    parent: i64,
    kind: i64,
    guid: String,
    title: Option<String>,
    url: Option<String>,
}

fn read_snapshot(db: &Path, tree: &mut Tree) -> Result<Vec<BookmarkNode>, Issue> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_| Issue::Invalid)?;
    conn.execute_batch(
        "PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; PRAGMA temp_store=MEMORY;",
    )
    .map_err(|_| Issue::Invalid)?;
    // 用外键取得书签 URL。浏览记录、计数及访问时间均不参与查询。
    let mut query = conn
        .prepare(
            "SELECT b.id, b.parent, b.type, substr(b.guid,1,128), substr(b.title,1,512),
           CASE WHEN length(p.url) <= ?1 THEN p.url END
         FROM moz_bookmarks b LEFT JOIN moz_places p ON b.type=1 AND p.id=b.fk
         WHERE b.type IN (1,2) ORDER BY b.parent,b.position,b.id LIMIT ?2",
        )
        .map_err(|_| Issue::Invalid)?;
    let limit = tree.remaining;
    let rows = query
        .query_map([MAX_URL as i64, limit as i64 + 2], |r| {
            Ok(Row {
                id: r.get(0)?,
                parent: r.get(1)?,
                kind: r.get(2)?,
                guid: r.get(3)?,
                title: r.get(4)?,
                url: r.get(5)?,
            })
        })
        .map_err(|_| Issue::Invalid)?;
    let mut by_id = HashMap::new();
    let mut children: HashMap<_, Vec<_>> = HashMap::new();
    for row in rows {
        let row = row.map_err(|_| Issue::Invalid)?;
        children.entry(row.parent).or_default().push(row.id);
        if by_id.insert(row.id, row).is_some() {
            return Err(Issue::Invalid);
        }
    }
    if by_id.len() > limit + 1 {
        return Err(Issue::Limit);
    }
    let root_id = by_id
        .values()
        .find(|row| row.guid == "root________" && row.kind == 2 && row.parent == 0)
        .ok_or(Issue::Invalid)?
        .id;
    let rows = Places { by_id, children };
    rows.children(root_id, "root", None, tree, 0)
}

struct Places {
    by_id: HashMap<i64, Row>,
    children: HashMap<i64, Vec<i64>>,
}

impl Places {
    fn children(
        &self,
        parent: i64,
        parent_id: &str,
        group: Option<&str>,
        tree: &mut Tree,
        depth: usize,
    ) -> Result<Vec<BookmarkNode>, Issue> {
        let mut nodes = Vec::new();
        for (index, id) in self.children.get(&parent).into_iter().flatten().enumerate() {
            let row = self.by_id.get(id).ok_or(Issue::Invalid)?;
            tree.visit(depth)?;
            // Tags 是虚拟分类，也包含指向相同 URL 的内部记录。
            if row.guid == "tags________" {
                continue;
            }
            let fallback_id = row.id.to_string();
            let native = Some(if row.guid.is_empty() {
                fallback_id.as_str()
            } else {
                row.guid.as_str()
            });
            if row.kind == 1 {
                if let Some(node) = row.url.as_deref().and_then(|url| {
                    tree.bookmark(native, parent_id, index, row.title.as_deref(), url, group)
                }) {
                    nodes.push(node);
                }
                continue;
            }
            let id = tree.id(native, parent_id, index);
            let fallback = match row.guid.as_str() {
                "toolbar_____" => "书签工具栏",
                "menu________" => "书签菜单",
                "unfiled_____" => "其他书签",
                "mobile______" => "移动设备书签",
                _ => "书签文件夹",
            };
            let name = label(row.title.as_deref(), fallback);
            let children = self.children(row.id, &id, Some(&name), tree, depth + 1)?;
            nodes.push(BookmarkNode::Folder { id, name, children });
        }
        Ok(nodes)
    }
}
