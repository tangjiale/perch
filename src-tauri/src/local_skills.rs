//! 本机技能仅作为文本导入，不执行技能中的脚本或指令。
use crate::{storage::Store, AppState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tauri::State;

const MAX_BYTES: u64 = 256 * 1024;
const MAX_SKILLS: usize = 500;
const MAX_ENTRIES: usize = 5000;
const MAX_DEPTH: usize = 6;

fn external_skill(value: &Value) -> bool {
    value["source"] == "local"
        || value["sourcePath"]
            .as_str()
            .is_some_and(|path| !path.is_empty())
        || value["sourceGroup"]
            .as_str()
            .is_some_and(|group| !group.is_empty() && group != "perch")
}

fn validate_edit(old: Option<&Value>, value: &Value) -> Result<(), String> {
    if let Some(old) = old.filter(|old| external_skill(old)) {
        let mut before = old.clone();
        let mut after = value.clone();
        before
            .as_object_mut()
            .ok_or("技能数据无效")?
            .remove("enabled");
        after
            .as_object_mut()
            .ok_or("技能数据无效")?
            .remove("enabled");
        if before != after {
            return Err("外部来源技能只能查看内容和启用／停用，不能修改技能信息".into());
        }
    } else if external_skill(value) {
        return Err("外部来源技能必须通过读取本机技能导入".into());
    }
    Ok(())
}

#[tauri::command]
pub fn save_skill(state: State<'_, AppState>, value: Value) -> Result<Value, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let snapshot = store.snapshot()?;
    let old = snapshot["skills"]
        .as_array()
        .and_then(|skills| skills.iter().find(|skill| skill["id"] == value["id"]));
    validate_edit(old, &value)?;
    store.save("skill", value)
}

#[tauri::command]
pub fn delete_skill(
    state: State<'_, AppState>,
    id: String,
    revision: Option<i64>,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let snapshot = store.snapshot()?;
    if snapshot["skills"].as_array().is_some_and(|skills| {
        skills
            .iter()
            .any(|skill| skill["id"] == id && external_skill(skill))
    }) {
        return Err("外部来源技能不能在栖点删除，可将其停用".into());
    }
    store.delete("skill", &id, revision)
}

#[derive(Default, Serialize)]
pub struct ScanSummary {
    added: usize,
    existing: usize,
    warnings: Vec<String>,
    skipped: usize,
}

fn parse_skill(path: &Path) -> Result<Value, ()> {
    let metadata = fs::metadata(path).map_err(|_| ())?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(());
    }
    let file = fs::File::open(path).map_err(|_| ())?;
    if !file.metadata().map_err(|_| ())?.is_file() {
        return Err(());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(());
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| ())?
        .trim_start_matches('\u{feff}');
    let normalized = text.replace("\r\n", "\n");
    let rest = normalized.strip_prefix("---\n").ok_or(())?;
    let (header, content) = rest.split_once("\n---\n").ok_or(())?;
    let metadata: serde_yaml::Value = serde_yaml::from_str(header).map_err(|_| ())?;
    let name = metadata["name"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or(())?;
    let description = metadata["description"].as_str().unwrap_or("");
    let id = format!(
        "local-skill-{:x}",
        Sha256::digest(path.as_os_str().as_encoded_bytes())
    );
    Ok(
        json!({"id":id,"name":name.trim(),"description":description.trim(),"content":content.trim(),"enabled":false,"source":"local","sourcePath":path.to_string_lossy()}),
    )
}

fn scan(roots: &[(PathBuf, &str)]) -> (Vec<Value>, usize) {
    let mut pending: Vec<_> = roots
        .iter()
        .rev()
        .map(|(p, group)| (p.clone(), 0, *group))
        .collect();
    let mut visited = HashSet::new();
    let mut files = HashSet::new();
    let mut skills = Vec::new();
    let mut skipped = 0;
    let mut entries = 0;
    while let Some((directory, depth, group)) = pending.pop() {
        if entries >= MAX_ENTRIES || skills.len() >= MAX_SKILLS {
            skipped += 1;
            break;
        }
        let Ok(canonical) = directory.canonicalize() else {
            continue;
        };
        if !visited.insert(canonical.clone()) {
            continue;
        }
        let skill = canonical.join("SKILL.md");
        if skill.exists() {
            if let Ok(path) = skill.canonicalize() {
                if files.insert(path.clone()) {
                    match parse_skill(&path) {
                        Ok(mut value) => {
                            value["sourceGroup"] = json!(group);
                            skills.push(value);
                        }
                        Err(()) => skipped += 1,
                    }
                }
            } else {
                skipped += 1;
            }
            // 技能包下的 scripts/references 等资源不属于其他技能扫描范围。
            continue;
        }
        if depth >= MAX_DEPTH {
            skipped += 1;
            continue;
        }
        let Ok(children) = fs::read_dir(&canonical) else {
            skipped += 1;
            continue;
        };
        let mut directories = Vec::new();
        for entry in children {
            entries += 1;
            if entries > MAX_ENTRIES {
                skipped += 1;
                break;
            }
            let Ok(entry) = entry else {
                skipped += 1;
                continue;
            };
            let path = entry.path();
            if path.is_dir() {
                directories.push(path);
            }
        }
        directories.sort();
        pending.extend(directories.into_iter().rev().map(|p| (p, depth + 1, group)));
    }
    (skills, skipped)
}

fn import(store: &Store, skills: Vec<Value>, skipped: usize) -> Result<ScanSummary, String> {
    let existing: HashMap<String, String> = {
        let conn = store.conn.lock().map_err(|e| e.to_string())?;
        let mut statement = conn
            .prepare("SELECT id,data FROM skills")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?
    };
    let mut result = ScanSummary {
        skipped,
        ..Default::default()
    };
    let mut changes = Vec::new();
    for skill in skills {
        if let Some(previous) = existing.get(skill["id"].as_str().ok_or("技能 ID 无效")?) {
            result.existing += 1;
            let mut previous: Value = serde_json::from_str(previous).map_err(|e| e.to_string())?;
            if previous
                .get("sourceGroup")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                previous["sourceGroup"] = skill["sourceGroup"].clone();
                changes.push(("skill", previous));
            }
        } else {
            changes.push(("skill", skill));
            result.added += 1;
        }
    }
    if skipped > 0 {
        result.warnings.push(format!(
            "有 {skipped} 项文件或目录因格式、权限、大小或扫描上限被跳过"
        ));
    }
    store.save_batch(changes, vec![], false)?;
    Ok(result)
}

#[tauri::command]
pub fn local_skills_import(state: State<'_, AppState>) -> Result<ScanSummary, String> {
    let generation = state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .generation
        .clone();
    let home = dirs::home_dir().ok_or("无法确定用户目录")?;
    let roots = [
        (".codex/skills", "codex"),
        (".agents/skills", "agents"),
        (".claude/skills", "claude"),
    ]
    .map(|(p, group)| (home.join(p), group));
    let (skills, skipped) = scan(&roots);
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.generation != generation {
        return Err("工作空间已切换，请重新读取本机技能".into());
    }
    import(&store, skills, skipped)
}

#[derive(Deserialize)]
pub struct SkillRevision {
    id: String,
    revision: i64,
}

fn set_enabled(store: &Store, items: Vec<SkillRevision>, enabled: bool) -> Result<usize, String> {
    if items.is_empty() || items.len() > MAX_SKILLS {
        return Err("请选择 1 至 500 个技能".into());
    }
    let mut selected = HashMap::new();
    for item in items {
        if item.id.trim().is_empty() || item.revision < 1 {
            return Err("技能 ID 或版本无效".into());
        }
        if selected
            .insert(item.id, item.revision)
            .is_some_and(|old| old != item.revision)
        {
            return Err("同一技能的版本不一致，请刷新后重试".into());
        }
    }
    let mut changes = Vec::new();
    let mut expected = Vec::new();
    {
        let conn = store.conn.lock().map_err(|e| e.to_string())?;
        for (id, revision) in selected {
            let (actual, data): (i64, String) = conn
                .query_row("SELECT revision,data FROM skills WHERE id=?1", [&id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .map_err(|_| "技能已不存在，请刷新后重试")?;
            if revision != actual {
                return Err("技能已被修改，请刷新后重试".into());
            }
            let mut value: Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;
            value["enabled"] = json!(enabled);
            expected.push(("skill", id, revision));
            changes.push(("skill", value));
        }
    }
    // 批量保存再次校验所有版本；并发修改或删除时整批回滚。
    Ok(store.save_batch(changes, expected, false)?.len())
}

#[tauri::command]
pub fn local_skills_set_enabled(
    state: State<'_, AppState>,
    skills: Vec<SkillRevision>,
    enabled: bool,
) -> Result<usize, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    set_enabled(&store, skills, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_skills_only_allow_enabled_changes() {
        let old = json!({"id":"external","revision":1,"sourcePath":"/skills/demo/SKILL.md","name":"名称","content":"正文","enabled":false});
        let mut next = old.clone();
        next["enabled"] = json!(true);
        assert!(validate_edit(Some(&old), &next).is_ok());
        next["content"] = json!("改写");
        assert!(validate_edit(Some(&old), &next).is_err());
        next = old.clone();
        next.as_object_mut().unwrap().remove("sourcePath");
        assert!(validate_edit(Some(&old), &next).is_err());
        assert!(validate_edit(None, &old).is_err());
        assert!(validate_edit(None, &json!({"id":"own","name":"自建","content":"正文"})).is_ok());
    }
    fn write_skill(root: &Path, name: &str) -> PathBuf {
        let directory = root.join(name);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("SKILL.md");
        fs::write(
            &path,
            "---\nname: 示例技能\ndescription: >\n  第一行\n  第二行\n---\n正文内容",
        )
        .unwrap();
        path
    }
    #[test]
    fn parses_yaml_and_preserves_existing_edits() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        let path = write_skill(&root, "example");
        let store = Store::open_at(temp.path().join("workspace")).unwrap();
        let (skills, skipped) = scan(&[(root.clone(), "codex")]);
        assert_eq!(skills[0]["description"], "第一行 第二行");
        assert_eq!(skills[0]["enabled"], false);
        let id = skills[0]["id"].clone();
        assert_eq!(import(&store, skills, skipped).unwrap().added, 1);
        let mut value = store.snapshot().unwrap()["skills"][0].clone();
        value["enabled"] = json!(true);
        value["content"] = json!("用户编辑");
        store.save("skill", value).unwrap();
        fs::write(path, "---\nname: changed\n---\n新内容").unwrap();
        let (skills, skipped) = scan(&[(root, "codex")]);
        assert_eq!(skills[0]["id"], id);
        assert_eq!(import(&store, skills, skipped).unwrap().existing, 1);
        let value = &store.snapshot().unwrap()["skills"][0];
        assert_eq!(value["enabled"], true);
        assert_eq!(value["content"], "用户编辑");
    }
    #[test]
    fn reimport_backfills_group_without_changing_edits() {
        let temp = tempfile::tempdir().unwrap();
        write_skill(temp.path(), "example");
        let store = Store::open_at(temp.path().join("workspace")).unwrap();
        let (skills, _) = scan(&[(temp.path().into(), "agents")]);
        let mut old = skills[0].clone();
        old.as_object_mut().unwrap().remove("sourceGroup");
        old["content"] = json!("已编辑");
        old["enabled"] = json!(true);
        store.save("skill", old).unwrap();
        let summary = import(&store, skills, 0).unwrap();
        assert_eq!(summary.added, 0);
        assert_eq!(summary.existing, 1);
        let saved = &store.snapshot().unwrap()["skills"][0];
        assert_eq!(saved["sourceGroup"], "agents");
        assert_eq!(saved["content"], "已编辑");
        assert_eq!(saved["enabled"], true);
    }

    #[test]
    fn bulk_toggle_validates_all_revisions_and_deduplicates() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_at(temp.path().into()).unwrap();
        for id in ["one", "two"] {
            store
                .save(
                    "skill",
                    json!({"id":id,"name":id,"content":"正文","enabled":false}),
                )
                .unwrap();
        }
        let item = |id: &str, revision| SkillRevision {
            id: id.into(),
            revision,
        };
        assert!(set_enabled(&store, vec![item("one", 1), item("two", 2)], true).is_err());
        assert!(store.snapshot().unwrap()["skills"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["enabled"] == false));
        assert!(set_enabled(&store, vec![item("one", 1), item("missing", 1)], true).is_err());
        assert!(set_enabled(&store, vec![], true).is_err());
        assert_eq!(
            set_enabled(
                &store,
                vec![item("one", 1), item("two", 1), item("one", 1)],
                true
            )
            .unwrap(),
            2
        );
        assert!(store.snapshot().unwrap()["skills"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["enabled"] == true));
        assert_eq!(
            set_enabled(&store, vec![item("one", 2), item("two", 2)], false).unwrap(),
            2
        );
    }

    #[cfg(unix)]
    #[test]
    fn linked_skill_keeps_scanned_source_group() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("external");
        write_skill(&original, "sample");
        let root = temp.path().join("codex");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(original.join("sample"), root.join("alias")).unwrap();
        let (skills, _) = scan(&[(root, "codex"), (original, "claude")]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0]["sourceGroup"], "codex");
    }

    #[test]
    fn rejects_large_and_invalid_documents() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_skill(temp.path(), "large");
        fs::write(path, vec![b'a'; MAX_BYTES as usize + 1]).unwrap();
        let invalid = write_skill(temp.path(), "invalid");
        fs::write(invalid, [0xff]).unwrap();
        let (skills, skipped) = scan(&[(temp.path().into(), "codex")]);
        assert!(skills.is_empty());
        assert_eq!(skipped, 2);
    }
    #[cfg(unix)]
    #[test]
    fn directory_links_deduplicate_and_cycles_terminate() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        write_skill(&root, "original");
        std::os::unix::fs::symlink(root.join("original"), root.join("alias")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("cycle")).unwrap();
        let (skills, skipped) = scan(&[(root, "codex")]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skipped, 0);
    }
}
