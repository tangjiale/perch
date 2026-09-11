# 当前实现共享契约

此文件用于并行实施对齐，不替代架构要求。正式类型见 src/lib/types.ts。

- Root 负责 package.json / Cargo.toml / Tauri lib.rs / React shell / 公共样式与 src/lib。
- Rust AppState 定义在 lib.rs：pub struct AppState { pub store: std::sync::Mutex<storage::Store> }。
- storage 模块提供 Store::open_default()->Result<Store,String>，snapshot()->Result<serde_json::Value,String>，save(kind:&str,value:Value)->Result<Value,String>，delete(kind:&str,id:&str,revision:Option<i64>)->Result<(),String>，info()->Value。内部按领域白名单处理，前端没有通用 SQL 或任意表名命令。
- 公共 Tauri 命令为 workspace_snapshot，storage_info，save_task / delete_task 等逐领域 wrappers，输入 {value} 或 {id,revision}。
- 前端页面 default export，props 为 PageProps；业务访问 api.save(kind,value)、api.remove(kind,id,revision)、command(name,args)。完成写入 await refresh()；错误 notify(error.message)。
- 共享 Snapshot 的数组和字段以 types.ts 为准；任务 status 为 todo/doing/done/closed、source local/zentao。schedule {kind,timezone,start,end?}，timed 为带偏移的 ISO datetime，all_day 为日期且 end exclusive。
- 各模块只能编辑分配文件。需要变更共享契约先通知 root。
