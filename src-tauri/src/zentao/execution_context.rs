//! 只读执行信息与附件操作共用的本地关联、凭据和工作空间校验。
use super::*;

pub(super) struct Context {
    pub(super) task: Value,
    pub(super) connection: Value,
    pub(super) project: String,
    pub(super) workspace: String,
    pub(super) generation: std::path::PathBuf,
}

pub(super) fn context(state: &AppState, task_id: &str) -> Result<Context, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let snapshot = store.snapshot()?;
    let task = get(&snapshot, "tasks", task_id)?;
    if task["source"] != "zentao" || task["remoteType"] != "execution" {
        return Err("请先保存并同步禅道执行".into());
    }
    let connection = get(&snapshot, "connections", text(&task, "connectionId"))?;
    if connection["enabled"] == false {
        return Err("禅道连接已停用".into());
    }
    let project = get(&snapshot, "projects", text(&task, "projectId"))?;
    if project["source"] != "zentao" || project["connectionId"] != task["connectionId"] {
        return Err("执行的禅道项目关联无效".into());
    }
    Ok(Context {
        task,
        connection,
        project: remote_id(&project["remoteId"])?,
        workspace: store.workspace_id.clone(),
        generation: store.generation.clone(),
    })
}

pub(super) fn unchanged(state: &AppState, ctx: &Context) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != ctx.workspace || store.generation != ctx.generation {
        return Err("数据目录已切换，请重新打开任务".into());
    }
    let snapshot = store.snapshot()?;
    let connection = get(&snapshot, "connections", text(&ctx.connection, "id"))?;
    let task = get(&snapshot, "tasks", text(&ctx.task, "id"))?;
    if connection["revision"] != ctx.connection["revision"]
        || task["revision"] != ctx.task["revision"]
    {
        return Err("连接或任务已变化，请重新打开任务".into());
    }
    Ok(())
}

pub(super) fn adapter<'a>(state: &'a AppState, ctx: &Context) -> Result<Adapter<'a>, String> {
    let token = credential(&ctx.workspace, text(&ctx.connection, "id"))?
        .get_password()
        .map_err(|_| "请先保存禅道令牌")?;
    Ok(
        Adapter::personal(&ctx.connection, token)?.with_auth(crate::zentao_auth::AutoAuth::new(
            state,
            &ctx.workspace,
            &ctx.generation,
            &ctx.connection,
        )?),
    )
}
