use super::*;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn source(layout: Layout, browser: &str, home: &Path, config: &Path, local: &Path) -> Source {
    sources(layout, Some(home), Some(config), Some(local))
        .into_iter()
        .find(|source| source.id == browser)
        .unwrap()
}

fn run(source: Source) -> Value {
    let mut remaining = MAX_TOTAL_NODES;
    serde_json::to_value(scan_browser(source, &mut remaining)).unwrap()
}

fn chromium_file(path: &Path, caption: &str) {
    let root = json!({"checksum":"sample", "version":1, "roots": {
        "bookmark_bar": {"children":[
            {"children":[
                {"date_added":"13370146529000000","guid":"b62d0513-bdff-41ad-83d2-8987803a2c51","id":"7","name":caption,"type":"url","url":"https://example.test/nested?a=1#tab"},
                {"id":"8","name":"脚本","type":"url","url":"javascript:alert(1)"},
                {"id":"9","name":"凭据","type":"url","url":"https://user:secret@example.test/"}
            ],"guid":"3d9c5030-f8a9-4f4f-b9f7-4e55dd9df013","id":"6","name":"项目","type":"folder"},
            {"id":"10","name":"Wiki","type":"url","url":"http://example.test/wiki"}
        ],"guid":"00000000-0000-4000-a000-000000000002","id":"1","name":"Bookmarks bar","type":"folder"},
        "other": {"children":[],"guid":"00000000-0000-4000-a000-000000000003","id":"2","type":"folder"},
        "synced": {"children":[],"guid":"00000000-0000-4000-a000-000000000004","id":"3","type":"folder"}
    }});
    write(path, root.to_string());
}

#[test]
fn chrome_and_edge_read_standard_locations_and_independent_profiles() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let mac_config = home.join("Library/Application Support");
    let local = home.join("AppData/Local");
    let roaming = home.join("AppData/Roaming");
    for (layout, name, config, path) in [
        (
            Layout::Mac,
            "chrome",
            &mac_config,
            mac_config.join("Google/Chrome"),
        ),
        (
            Layout::Mac,
            "edge",
            &mac_config,
            mac_config.join("Microsoft Edge"),
        ),
        (
            Layout::Windows,
            "chrome",
            &roaming,
            local.join("Google/Chrome/User Data"),
        ),
        (
            Layout::Windows,
            "edge",
            &roaming,
            local.join("Microsoft/Edge/User Data"),
        ),
    ] {
        let bookmark_file = path.join("Default/Bookmarks");
        chromium_file(&bookmark_file, "文档");
        chromium_file(&path.join("Profile 2/Bookmarks"), "同名 ID");
        chromium_file(&path.join("Profile 2/AccountBookmarks"), "账号文档");
        write(&path.join("Profile 3/Bookmarks"), b"{invalid json");
        write(
            &path.join("Default/Login Data"),
            b"fixture credentials marker",
        );
        write(&path.join("Local State"), b"fixture profile and key marker");
        let original = fs::read(&bookmark_file).unwrap();

        let result = run(source(layout, name, home, config, &local));
        assert_eq!(result["id"], name);
        assert_eq!(result["status"], "ready");
        assert!(result["message"].as_str().unwrap().contains("部分浏览配置"));
        let profiles = result["profiles"].as_array().unwrap();
        assert_eq!(profiles.len(), 3);
        assert_eq!(profiles[0]["name"], "Default");
        assert_eq!(profiles[1]["name"], "Profile 2");
        assert_eq!(profiles[2]["name"], "Profile 2 · 账号书签");
        let toolbar = &profiles[0]["nodes"][0];
        let project = &toolbar["children"][0];
        assert_eq!(project["kind"], "folder");
        assert_eq!(project["children"].as_array().unwrap().len(), 1);
        assert_eq!(project["children"][0]["name"], "文档");
        assert_eq!(project["children"][0]["group"], "项目");
        assert_eq!(toolbar["children"][1]["group"], "Bookmarks bar");
        assert_eq!(profiles[0]["nodes"][1]["name"], "其他书签");
        let ids: HashSet<_> = profiles
            .iter()
            .map(|profile| {
                profile["nodes"][0]["children"][0]["children"][0]["id"]
                    .as_str()
                    .unwrap()
            })
            .collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(result, run(source(layout, name, home, config, &local)));
        assert_eq!(fs::read(&bookmark_file).unwrap(), original);
        assert!(!result
            .to_string()
            .contains(&home.to_string_lossy().to_string()));
    }
}

fn create_places(path: &Path) -> Connection {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER DEFAULT 0);
         CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER, parent INTEGER, position INTEGER, title TEXT, guid TEXT);
         CREATE INDEX moz_bookmarks_parentindex ON moz_bookmarks(parent, position);
         CREATE TABLE moz_historyvisits (id INTEGER PRIMARY KEY, place_id INTEGER, visit_date INTEGER);
         INSERT INTO moz_places (id,url,title) VALUES (1,'https://example.test/manual','History title'), (2,'https://history-only.invalid/','不可见历史'), (3,'about:config',NULL);
         INSERT INTO moz_historyvisits VALUES (1,2,13370146529000000);
         INSERT INTO moz_bookmarks VALUES
           (1,2,NULL,0,0,NULL,'root________'),
           (2,2,NULL,1,0,NULL,'menu________'),
           (3,2,NULL,1,1,NULL,'toolbar_____'),
           (4,2,NULL,1,2,NULL,'tags________'),
           (5,2,NULL,1,3,NULL,'unfiled_____'),
           (6,2,NULL,1,4,NULL,'mobile______'),
           (7,2,NULL,3,0,'研究','folder123456'),
           (8,1,1,7,0,'手册','bookmark0001'),
           (9,3,NULL,7,1,NULL,'separator001'),
           (10,1,3,7,2,'内部页','bookmark0002'),
           (11,2,NULL,4,0,'标签','tagfolder001'),
           (12,1,1,11,0,'标签引用','tagbookmark1');",
    ).unwrap();
    conn
}

fn file_contents(directory: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().into_string().unwrap(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn firefox_reads_wal_and_profiles_ini_without_touching_places_files() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let config = home.join("Library/Application Support");
    let profiles = config.join("Firefox");
    let work = profiles.join("Profiles/01234567.work");
    let personal = home.join("outside-profile-root/personal");
    let discovered = profiles.join("Profiles/abcdef12.fallback");
    write(&profiles.join("profiles.ini"), format!(
        "\u{feff}[Profile0]\r\nName=工作\r\nIsRelative=1\r\nPath=Profiles/01234567.work\r\n\r\n[Profile1]\r\nName=个人\r\nIsRelative=0\r\nPath={}\r\n[General]\r\nVersion=2\r\n",
        personal.display(),
    ));
    let writer = create_places(&work.join("places.sqlite"));
    writer
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    writer
        .execute(
            "INSERT INTO moz_places(id,url) VALUES(4,?1)",
            ["https://example.test/wal"],
        )
        .unwrap();
    writer
        .execute(
            "INSERT INTO moz_bookmarks VALUES(13,1,4,2,0,?1,?2)",
            params!["活跃 WAL", "walbookmark1"],
        )
        .unwrap();
    drop(create_places(&personal.join("places.sqlite")));
    drop(create_places(&discovered.join("places.sqlite")));
    let active_before = file_contents(&work);
    let closed_before = file_contents(&personal);
    assert!(!active_before["places.sqlite-wal"].is_empty());

    let result = run(source(Layout::Mac, "firefox", home, &config, home));
    assert_eq!(result["status"], "ready");
    let profiles = result["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 3);
    let profile = profiles
        .iter()
        .find(|profile| profile["name"] == "工作")
        .unwrap();
    assert_eq!(profile["nodes"][0]["children"][0]["name"], "活跃 WAL");
    assert_eq!(profile["nodes"][0]["children"][0]["group"], "书签菜单");
    assert_eq!(
        profile["nodes"][1]["children"][0]["children"][0]["group"],
        "研究"
    );
    assert!(profiles.iter().any(|profile| profile["name"] == "个人"));
    assert!(profiles
        .iter()
        .any(|profile| profile["name"] == "abcdef12.fallback"));
    assert!(!result.to_string().contains("history-only.invalid"));
    assert!(!result.to_string().contains("标签引用"));
    assert_eq!(file_contents(&work), active_before);
    assert_eq!(file_contents(&personal), closed_before);
    drop(writer);
}

#[test]
fn firefox_finds_desktop_and_msix_profiles_on_windows_and_standard_unix_profiles() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let config = home.join("AppData/Roaming");
    let local = home.join("AppData/Local");
    for path in [
        config.join("Mozilla/Firefox/Profiles/desktop.default/places.sqlite"),
        local.join("Packages/Mozilla.Firefox_n80bbvh6b1yt2/LocalCache/Roaming/Mozilla/Firefox/Profiles/store.default/places.sqlite"),
        home.join(".mozilla/firefox/unix.default/places.sqlite"),
    ] {
        drop(create_places(&path));
    }
    let result = run(source(Layout::Windows, "firefox", home, &config, &local));
    let windows = result["profiles"].as_array().unwrap();
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0]["name"], "desktop.default");
    assert_eq!(windows[1]["name"], "store.default");
    let unix = run(source(Layout::Unix, "firefox", home, &config, &local));
    assert_eq!(unix["profiles"][0]["name"], "unix.default");
}

const SAFARI: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>WebBookmarkFileVersion</key><integer>1</integer>
  <key>Children</key><array>
    <dict><key>WebBookmarkType</key><string>WebBookmarkTypeList</string>
      <key>Title</key><string>BookmarksBar</string><key>WebBookmarkUUID</key><string>AFF63122-A000-46CC-AB5D-48E01883BE4A</string>
      <key>Children</key><array>
        <dict><key>WebBookmarkType</key><string>WebBookmarkTypeList</string><key>Title</key><string>工作收藏</string>
          <key>Children</key><array>
            <dict><key>WebBookmarkType</key><string>WebBookmarkTypeLeaf</string>
              <key>WebBookmarkUUID</key><string>5AC4AECA-F68B-4742-98BA-88E71E53A3B0</string>
              <key>URLString</key><string>https://example.test/a?x=1&amp;y=2</string>
              <key>URIDictionary</key><dict><key>title</key><string>Safari 书签</string></dict></dict>
          </array></dict>
      </array></dict>
    <dict><key>WebBookmarkType</key><string>WebBookmarkTypeLeaf</string>
      <key>URLString</key><string>http://example.test/top</string>
      <key>URIDictionary</key><dict><key>title</key><string>顶层</string></dict></dict>
    <dict><key>WebBookmarkType</key><string>WebBookmarkTypeList</string><key>Title</key><string>com.apple.ReadingList</string></dict>
  </array>
</dict></plist>"#;

#[test]
fn safari_plist_preserves_nested_bookmarks_and_null_group() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let path = home.join("Library/Safari/Bookmarks.plist");
    write(&path, SAFARI);
    let result = run(source(Layout::Mac, "safari", home, home, home));
    assert_eq!(result["status"], "ready");
    let nodes = result["profiles"][0]["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0]["name"], "个人收藏");
    let nested = &nodes[0]["children"][0]["children"][0];
    assert_eq!(nested["name"], "Safari 书签");
    assert_eq!(nested["group"], "工作收藏");
    assert_eq!(nested["url"], "https://example.test/a?x=1&y=2");
    assert_eq!(nodes[1]["kind"], "bookmark");
    assert!(nodes[1]["group"].is_null());
    assert_eq!(fs::read_to_string(&path).unwrap(), SAFARI);
}

#[cfg(target_os = "macos")]
#[test]
fn safari_binary_plist_is_converted_read_only() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let path = home.join("Library/Safari/Bookmarks.plist");
    write(&path, SAFARI);
    assert!(std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "binary1", "--"])
        .arg(&path)
        .status()
        .unwrap()
        .success());
    let original = fs::read(&path).unwrap();
    assert!(original.starts_with(b"bplist00"));
    let result = run(source(Layout::Mac, "safari", home, home, home));
    assert_eq!(result["status"], "ready");
    assert_eq!(result["profiles"][0]["nodes"][1]["name"], "顶层");
    assert_eq!(original, fs::read(&path).unwrap());
}

#[test]
fn scanner_distinguishes_missing_and_damaged_without_exposing_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let local = home.join("Local");
    let config = home.join("Roaming");
    let missing = run(source(Layout::Windows, "edge", home, &config, &local));
    assert_eq!(missing["status"], "unavailable");
    let store =
        local.join("Packages/Mozilla.Firefox_n80bbvh6b1yt2/LocalCache/Roaming/Mozilla/Firefox");
    write(
        &store.join("Profiles/corrupt/places.sqlite"),
        b"not a database",
    );
    let broken = run(source(Layout::Windows, "firefox", home, &config, &local));
    assert_eq!(broken["status"], "error");
    assert!(broken["message"].as_str().unwrap().contains("格式无效"));
    assert!(!broken.to_string().contains("corrupt"));
    assert_eq!(broken["profiles"], json!([]));
    let path = home.join("Library/Safari/Bookmarks.plist");
    write(
        &path,
        b"<!DOCTYPE plist [<!ENTITY ext SYSTEM 'file:///fixture-only'>]><plist>&ext;</plist>",
    );
    assert_eq!(
        run(source(Layout::Mac, "safari", home, &config, &local))["status"],
        "error"
    );
}

#[cfg(unix)]
#[test]
fn permission_denied_and_symlink_bookmark_sources_are_not_returned() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let config = root.join("config");
    let profile = config.join("google-chrome/Default");
    let path = profile.join("Bookmarks");
    chromium_file(&path, "不应返回");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if File::open(&path).is_err() {
        let gated = run(source(Layout::Unix, "chrome", root, &config, root));
        assert_eq!(gated["status"], "error");
        assert!(gated["message"]
            .as_str()
            .unwrap()
            .contains("读取书签文件的权限"));
    }
    let outside = root.join("outside.json");
    fs::rename(&path, &outside).unwrap();
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
    let bytes = fs::read(&outside).unwrap();
    symlink(&outside, &path).unwrap();
    let linked = run(source(Layout::Unix, "chrome", root, &config, root));
    assert_eq!(linked["status"], "error");
    assert_eq!(linked["profiles"], json!([]));
    assert_eq!(fs::read(&outside).unwrap(), bytes);
}
