use super::*;

#[derive(serde::Serialize, Debug, PartialEq)]
pub struct Assignee {
    account: String,
    realname: String,
}

pub(super) fn user_account(row: &Value) -> Result<&str, String> {
    row["account"]
        .as_str()
        .filter(|account| {
            !account.trim().is_empty()
                && account.len() <= 256
                && !account.chars().any(char::is_control)
        })
        .ok_or_else(|| "禅道人员列表包含无效账号".into())
}

fn available(row: &Value) -> bool {
    // 官方接口排除已删除账号；兼容扩展响应中的显式停用标志。
    !["deleted", "disabled"]
        .iter()
        .any(|key| row[*key] == true || row[*key] == 1 || row[*key] == "1")
        && row["enabled"] != false
        && row["status"] != "disabled"
}

pub(super) async fn read_assignees(adapter: &Adapter<'_>) -> Result<Vec<Assignee>, String> {
    adapter.account().await?;
    adapter
        .all("users", "users")
        .await?
        .into_iter()
        .filter(available)
        .map(|row| {
            let account = user_account(&row)?.to_owned();
            let realname = row["realname"]
                .as_str()
                .ok_or("禅道人员列表缺少姓名字段")?
                .to_owned();
            Ok(Assignee { account, realname })
        })
        .collect()
}

struct Context {
    workspace: String,
    generation: std::path::PathBuf,
    connection: Value,
    bug: Value,
}
fn context(state: &AppState, id: &str) -> Result<Context, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let snapshot = store.snapshot()?;
    let bug = get(&snapshot, "bugs", id)?;
    remote_id(&bug["remoteId"])?;
    let connection = get(&snapshot, "connections", text(&bug, "connectionId"))?;
    if connection["enabled"] == false {
        return Err("该禅道连接已停用".into());
    }
    Ok(Context {
        workspace: store.workspace_id.clone(),
        generation: store.generation.clone(),
        connection,
        bug,
    })
}
fn unchanged(state: &AppState, ctx: &Context) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != ctx.workspace || store.generation != ctx.generation {
        return Err("工作空间已切换，请重新打开 BUG".into());
    }
    let snapshot = store.snapshot()?;
    let connection = get(&snapshot, "connections", text(&ctx.connection, "id"))?;
    let bug = get(&snapshot, "bugs", text(&ctx.bug, "id"))?;
    if connection != ctx.connection || bug != ctx.bug {
        return Err("连接或 BUG 已变化，请重新打开 BUG".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn zentao_bug_assignees(
    state: State<'_, AppState>,
    bug_id: String,
) -> Result<Vec<Assignee>, String> {
    let ctx = context(&state, &bug_id)?;
    let token = credential(&ctx.workspace, text(&ctx.connection, "id"))?
        .get_password()
        .map_err(|_| "请先保存禅道令牌")?;
    let adapter =
        Adapter::personal(&ctx.connection, token)?.with_auth(crate::zentao_auth::AutoAuth::new(
            &state,
            &ctx.workspace,
            &ctx.generation,
            &ctx.connection,
        )?);
    let result = read_assignees(&adapter).await?;
    unchanged(&state, &ctx)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zentao::tests::auth_server;
    fn adapter(base: &str) -> Adapter<'static> {
        Adapter::personal(&json!({"baseUrl":base,"apiVersion":"v2"}), "fixture".into()).unwrap()
    }
    fn profile() -> (u16, Value) {
        (200, json!({"profile":{"account":"me"}}))
    }
    #[tokio::test]
    async fn reads_official_account_keyed_pages_and_filters_explicit_unavailable_users() {
        let (base, server) = auth_server(vec![
            profile(),
            (
                200,
                json!({"page":1,"total":4,"limit":2,"users":[{"account":"me","realname":"唐家乐"},{"account":"other","realname":"同名用户"}]}),
            ),
            (
                200,
                json!({"page":2,"total":4,"limit":2,"users":[{"account":"gone","realname":"删除","deleted":"1"},{"account":"off","realname":"停用","enabled":false}]}),
            ),
        ]);
        let result = read_assignees(&adapter(&base)).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].realname, "唐家乐");
        assert_eq!(result[1].account, "other");
        let requests = server.join().unwrap();
        assert!(requests[1].starts_with("GET /api.php/v1/users?browse=all&type=bydept&page=1&limit=100&order=id_asc&status=all HTTP/1.1"));
        assert!(requests[2].contains("page=2&limit=100"));
        assert!(!requests[1].contains("full=1"));
    }
    #[tokio::test]
    async fn rejects_duplicate_invalid_pages_and_invalid_users() {
        for body in [
            json!({"page":2,"total":1,"limit":100,"users":[]}),
            json!({"page":1,"total":2,"limit":100,"users":[{"account":"me","realname":"甲"},{"account":"me","realname":"乙"}]}),
            json!({"page":1,"total":1,"limit":100,"users":[{"account":"","realname":"甲"}]}),
            json!({"page":1,"total":1,"limit":100,"users":[{"account":"me"}]}),
        ] {
            let (base, server) = auth_server(vec![profile(), (200, body)]);
            assert!(read_assignees(&adapter(&base)).await.is_err());
            server.join().unwrap();
        }
    }
    #[tokio::test]
    async fn duplicate_accounts_across_pages_abort_complete_result() {
        let row = json!({"account":"me","realname":"本人"});
        let (base, server) = auth_server(vec![
            profile(),
            (
                200,
                json!({"page":1,"total":2,"limit":1,"users":[row.clone()]}),
            ),
            (200, json!({"page":2,"total":2,"limit":1,"users":[row]})),
        ]);
        assert!(read_assignees(&adapter(&base))
            .await
            .unwrap_err()
            .contains("重复"));
        assert_eq!(server.join().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn authentication_and_permission_failures_are_not_empty_lists() {
        for status in [401, 403] {
            let (base, server) = auth_server(vec![(status, json!({"private":"hidden"}))]);
            let error = read_assignees(&adapter(&base)).await.unwrap_err();
            assert!(error.contains(&status.to_string()));
            assert!(!error.contains("hidden"));
            server.join().unwrap();
        }
    }
    #[test]
    fn invalidates_changed_connection_bug_and_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(tmp.path().join("workspace")).unwrap();
        store
            .save("connection", json!({"id":"c","name":"fixture"}))
            .unwrap();
        store
            .save_batch(
                vec![(
                    "bug",
                    json!({"id":"b","connectionId":"c","remoteId":"1","title":"fixture BUG","status":"active","assignedTo":"me","severity":2,"priority":3,"productId":"1","steps":""}),
                )],
                vec![],
                true,
            )
            .unwrap();
        let state = AppState {
            store: Mutex::new(store),
        };
        let ctx = context(&state, "b").unwrap();
        unchanged(&state, &ctx).unwrap();
        let mut changed = ctx.connection.clone();
        changed["name"] = json!("changed");
        state
            .store
            .lock()
            .unwrap()
            .save("connection", changed)
            .unwrap();
        assert!(unchanged(&state, &ctx).is_err());
        let ctx = context(&state, "b").unwrap();
        let mut bug = ctx.bug.clone();
        bug["title"] = json!("changed BUG");
        state
            .store
            .lock()
            .unwrap()
            .save_batch(vec![("bug", bug)], vec![], true)
            .unwrap();
        assert!(unchanged(&state, &ctx).is_err());
        state.store.lock().unwrap().generation = tmp.path().join("other");
        assert!(unchanged(&state, &ctx).is_err());
    }
}
