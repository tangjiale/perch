use crate::{data::text, storage::Store};
use serde_json::{json, Value};

// 外层 None 表示普通编辑，内层 None 表示拖至列尾。此字段不属于任务实体。
pub(super) type Placement = Option<Option<String>>;

pub(super) fn take_placement(value: &mut Value) -> Result<Placement, String> {
    match value
        .as_object_mut()
        .and_then(|object| object.remove("boardBeforeId"))
    {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(id)) if !id.is_empty() => Ok(Some(Some(id))),
        _ => Err("无效的看板插入位置".into()),
    }
}

pub(super) fn validate_placement(
    snapshot: &Value,
    value: &Value,
    placement: &Placement,
) -> Result<(), String> {
    let Some(before) = placement else {
        return Ok(());
    };
    let rows = snapshot["tasks"].as_array().ok_or("任务列表无效")?;
    let current = rows
        .iter()
        .find(|task| task["id"] == value["id"])
        .ok_or("新增任务不能指定看板插入位置")?;
    if current["revision"] != value["revision"] {
        return Err("任务已被修改，请刷新后重新拖动".into());
    }
    if !["todo", "doing", "done", "closed"].contains(&text(value, "status")) {
        return Err("无效的任务状态".into());
    }
    if let Some(before) = before {
        if before == text(value, "id")
            || !rows
                .iter()
                .any(|task| text(task, "id") == before && task["status"] == value["status"])
        {
            return Err("目标任务已移动或删除，请刷新后重新拖动".into());
        }
    }
    Ok(())
}

pub(super) fn save(store: &Store, value: Value, placement: &Placement) -> Result<Value, String> {
    if placement.is_none() {
        return store.save("task", value);
    }
    let snapshot = store.snapshot()?;
    validate_placement(&snapshot, &value, placement)?;
    let rows = snapshot["tasks"].as_array().ok_or("任务列表无效")?;
    let mut column: Vec<Value> = rows
        .iter()
        .filter(|task| task["status"] == value["status"] && task["id"] != value["id"])
        .cloned()
        .collect();
    // 同分数以 ID 稳定排序，与前端一致；整列重编号避免重复时间戳及浮点间隙耗尽。
    column.sort_by(|a, b| {
        a["sortOrder"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&b["sortOrder"].as_f64().unwrap_or(0.0))
            .then_with(|| text(a, "id").cmp(text(b, "id")))
    });
    let position = placement
        .as_ref()
        .and_then(|before| before.as_ref())
        .and_then(|id| column.iter().position(|task| text(task, "id") == id))
        .unwrap_or(column.len());
    let moving_id = text(&value, "id").to_string();
    column.insert(position, value);
    let mut expected = vec![];
    let mut changes = vec![];
    for (index, mut task) in column.into_iter().enumerate() {
        expected.push((
            "task",
            text(&task, "id").to_string(),
            task["revision"].as_i64().ok_or("任务缺少版本")?,
        ));
        let rank = (index as i64 + 1).checked_mul(1024).ok_or("任务数量过多")?;
        if text(&task, "id") == moving_id || task["sortOrder"].as_i64() != Some(rank) {
            task["sortOrder"] = json!(rank);
            changes.push(("task", task));
        }
    }
    // 兄弟任务只改本地顺序；任何版本或关联校验失败则整个事务回滚。
    store
        .save_batch(changes, expected, false)?
        .into_iter()
        .find(|task| text(task, "id") == moving_id)
        .ok_or("移动任务保存失败".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(store: &Store, id: &str, status: &str, order: i64) -> Value {
        store.save("task", json!({"id":id,"title":id,"source":"local","status":status,"sortOrder":order,"notes":"保留备注"})).unwrap()
    }
    fn order(store: &Store, status: &str) -> Vec<String> {
        let snapshot = store.snapshot().unwrap();
        let mut rows: Vec<_> = snapshot["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| text(row, "status") == status)
            .collect();
        rows.sort_by_key(|row| row["sortOrder"].as_i64().unwrap());
        rows.iter().map(|row| text(row, "id").to_string()).collect()
    }
    #[test]
    fn board_reorders_ties_across_columns_and_empty_column() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        task(&store, "a", "todo", 1);
        task(&store, "b", "todo", 1);
        let c = task(&store, "c", "todo", 1);
        let mut c = save(&store, c, &Some(Some("b".into()))).unwrap();
        assert_eq!(order(&store, "todo"), ["a", "c", "b"]);
        assert_eq!(c["notes"], "保留备注");
        c["status"] = json!("closed");
        let mut c = save(&store, c, &Some(None)).unwrap();
        assert_eq!(order(&store, "closed"), ["c"]);
        c["status"] = json!("todo");
        save(&store, c, &Some(Some("a".into()))).unwrap();
        assert_eq!(order(&store, "todo"), ["c", "a", "b"]);
    }
    #[test]
    fn board_rejects_invalid_anchor_and_stale_revision_without_changes() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let a = task(&store, "a", "todo", 1);
        task(&store, "b", "doing", 1);
        let before = store.snapshot().unwrap()["tasks"].clone();
        for id in ["a", "b", "missing"] {
            assert!(save(&store, a.clone(), &Some(Some(id.into()))).is_err());
        }
        let mut stale = a.clone();
        stale["revision"] = json!(0);
        assert!(save(&store, stale, &Some(None)).is_err());
        assert_eq!(store.snapshot().unwrap()["tasks"], before);
        assert!(save(&store, json!({"id":"new","status":"todo"}), &Some(None)).is_err());
    }
    #[test]
    fn board_placement_is_transient_and_sorting_has_no_remote_patch() {
        let mut value = json!({"id":"a","status":"todo","boardBeforeId":null});
        assert_eq!(take_placement(&mut value).unwrap(), Some(None));
        assert!(value.get("boardBeforeId").is_none());
        let old = value.clone();
        value["sortOrder"] = json!(1024);
        assert_eq!(
            super::super::execution_patch(&old, &value).unwrap(),
            json!({})
        );
    }

    #[test]
    fn board_batch_failure_rolls_back_siblings_and_skips_unchanged_ranks() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let a = task(&store, "a", "todo", 1024);
        let b = task(&store, "b", "todo", 1);
        let mut c = task(&store, "c", "todo", 1);
        let before = store.snapshot().unwrap()["tasks"].clone();
        c["title"] = json!("");
        assert!(save(&store, c.clone(), &Some(None)).is_err());
        assert_eq!(store.snapshot().unwrap()["tasks"], before);

        // 批量提交先验证所有参与记录的版本，避免只完成部分重排。
        let mut updated_b = b.clone();
        updated_b["notes"] = json!("并发修改");
        store.save("task", updated_b).unwrap();
        let before = store.snapshot().unwrap()["tasks"].clone();
        let mut changed_a = a.clone();
        changed_a["sortOrder"] = json!(999);
        assert!(store
            .save_batch(
                vec![("task", changed_a)],
                vec![("task", "b".into(), b["revision"].as_i64().unwrap())],
                false
            )
            .is_err());
        assert_eq!(store.snapshot().unwrap()["tasks"], before);

        // 移到原列尾时，已经具有目标分数的兄弟记录无需增加版本。
        c["title"] = json!("c");
        save(&store, c, &Some(None)).unwrap();
        let snapshot = store.snapshot().unwrap();
        let a_after = snapshot["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "a")
            .unwrap();
        // 初始顺序 b、a、c；a 的目标分数为 2048。
        let a_revision = a_after["revision"].clone();
        let c = snapshot["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "c")
            .unwrap()
            .clone();
        save(&store, c, &Some(None)).unwrap();
        assert_eq!(
            store.snapshot().unwrap()["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == "a")
                .unwrap()["revision"],
            a_revision
        );
    }
}
