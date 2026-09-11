# 数据模型、存储与迁移设计

状态：已进入实施的目标规格，实际交付与差异见 [实现记录](../IMPLEMENTATION.md)。适用：Tauri 2 + React + Rust，macOS 首发，保留 Windows 兼容。精确依赖见 package-lock.json 与 src-tauri/Cargo.lock。

## 1. 存储决策

| 方案 | 首版用途 | 决策依据 |
| --- | --- | --- |
| SQLite + rusqlite | 项目、任务、应用、模型、Agent、技能、会话、知识正文与索引元数据的唯一业务权威 | 单人本地桌面使用，无需安装数据库服务；具备事务、外键和版本迁移，便于一致备份 |
| 应用管理文件目录 | 上传原文件、Logo、可重建索引文件、临时文件 | 大二进制附件不塞入业务表；按内容哈希保存，不依赖用户原路径 |
| Markdown | 技能导入导出、可阅读业务导出、知识库原文件的一种格式 | 便于阅读和迁移，但不承担多表关系、状态并发和唯一性约束 |
| PostgreSQL | 未来可选服务端业务存储 | 在出现多用户或跨设备同步需求时引入；首版不要求用户部署 Postgres |

应用通过 Rust 应用服务和 Repository 在独立执行器上访问 rusqlite；React 不直接连接数据库，不拼 SQL。所有模块使用同一个业务数据库，不按页面分别存一份任务。数据库迁移版本、应用版本和数据导出格式版本分别维护，不能用一个数字替代。

首版数据边界是一个本地工作空间。保留工作空间 UUID，为导出和后续服务端导入标识来源；不在首版虚构租户权限或多端同步能力。默认数据根目录为 `~/.perch`。Rust 通过 Tauri 用户主目录 API 获取 home 后拼接 `.perch`，不硬编码用户名，不通过 shell 展开 `~`。首次启动时若发现旧版 `~/.self-workbanch` 且 `.perch` 不存在，会先将旧目录迁移为 `.perch`。

macOS 默认是 `/Users/<用户名>/.perch`；Windows 后续默认是用户 profile 目录下的 `.perch`。设置允许选择自定义本地目录，下面 `<data-root>` 指当前生效目录，默认即 `~/.perch`：

```text
<data-root>/
  current.json                 # 当前 workspace generation 的相对路径
  workspaces/<workspace-id>/
    generations/<generation-id>/
      workbench.sqlite3
      objects/sha256/ab/<hash>  # 不可变原文件和 Logo
      derived/                 # 可重建索引缓存
      staging/                 # 上传、解析和恢复中的临时文件
  backups/                     # 用户可另选受控导出路径
```

`current.json` 只承载工作空间定位，更新时使用同文件系统临时文件、刷盘、原子重命名；业务设置仍在 SQLite。generation 用于整库恢复切换，正常编辑不创建 generation。禁止两个 App 进程同时写同一个工作空间，启动时取得工作空间锁。

### 1.1 设置中的存储路径

入口为“设置 → 数据与备份 → 存储位置”。显示当前完整路径、默认/自定义标记、数据占用和目录可用状态；提供“选择目录”“在 Finder 中打开”“恢复默认路径”。Windows 使用系统文件管理器。

- 数据根目录统一管理 SQLite、文档原件、Logo、聊天图片、派生索引、暂存和默认备份。选择路径表示迁移现有数据，不是只修改后续上传文件的位置。
- 独立设置的外部备份目的地保持原配置，不随数据根目录隐式移动；界面明确区分工作数据位置与备份位置。
- 当前路径只读展示，使用原生目录选择器取得目标授权；选择后先检查写入/读取权限、空间、路径规范化以及与原目录的关系，再显示迁移预览。
- 禁止新旧路径相同、互为祖先/子目录或规范化后落在同一位置；首次迁移目标只接受空目录，不覆盖或合并已有文件。符号链接解析后再检查边界。
- 云盘同步目录和网络文件系统不用于实时 SQLite 工作空间；支持本地磁盘和满足锁、刷盘、原子重命名要求的外接卷。已配置卷离线或失去权限时进入目录恢复页，不能退回默认路径创建空库。
- 恢复默认路径同样执行迁移预览；默认目录已有旧工作空间时要求明确选择“打开已有工作空间”或另选空目标，不自动合并。打开已有空间须先校验完整性及版本，并与迁移当前空间区分。

### 1.2 启动定位配置

不能只在需要先打开的 SQLite 内保存 SQLite 所在目录。固定在 `~/.perch/bootstrap.json` 保存本机启动定位信息：`configVersion`、`dataRoot`、`workspaceId`。`dataRoot` 为规范化绝对路径；第一次明确初始化时默认指向 `~/.perch`，设置迁移成功后再切换。该文件不包含业务数据或凭据。

无论当前数据移到哪里，启动定位配置及应用级迁移锁始终留在 `~/.perch`；它们是明确的本机配置例外。选用自定义目录后，这个默认目录通常仍有很小的启动配置；迁移保留期内也可能保留旧工作数据。`bootstrap.json` 决定数据根目录，根目录内的 `current.json` 决定当前 generation，两者职责不同。

启动配置损坏、缺失且发现既有数据，或配置目录不可访问时，显示恢复/重新选择入口；创建全新空空间须是用户明确动作，不能静默初始化造成“数据消失”。启动配置不进入可携带数据包；换机恢复时由目标机写入自己的定位配置。密钥仍在系统 Keychain 中，路径变更不改变 workspace ID 或凭据槽 ID。

### 1.3 更换目录的提交与恢复

1. 生成迁移计划：记录当前目录、目标目录、空间需求及受影响的数据；目标仅选择尚不生效。取得应用级锁及工作空间锁，禁止同时恢复备份、升级 schema 或第二次迁移。
2. 用户确认后进入维护状态，停止新的写入、暂停 GC/同步/索引并等待事务结束；活动聊天先明确停止并落盘，结果未知的远端写保留未知状态，不能为迁移自动重放。
3. 通过一致备份生成 SQLite 快照，复制清单中的不可变原件、保留的工作空间 generations 与根内备份；可重建缓存允许重建。冻结期间直到切换完成都不接收新写入，避免复制结束前漏掉后续编辑。源目录若含 bootstrap/迁移锁，不复制它们作为目标启动配置。
4. 先写目标卷上的 staging，验证 SQLite 完整性、外键、实体数量、原件哈希及相对引用；刷盘后发布目标目录。跨卷使用复制与校验，不能把跨卷 rename 当成原子迁移。
5. 目标校验通过后，在固定启动目录内以临时写入、刷盘、原子替换 `bootstrap.json` 完成切换，保留旧配置副本和迁移日志。重新打开目标数据库成功后才更新界面生效路径，并恢复可恢复作业；不自动重发聊天或远端写请求。
6. 切换前失败保持原路径；切换后首次打开失败，依据持久迁移日志恢复旧配置并提示失败。迁移日志记录阶段、源/目标和校验结果，启动能判定使用哪一份数据。迁移过程中取消或断电不自动删除源数据。
7. 旧工作目录保留至新路径验证通过后，由用户另行确认清理；应用不能同时写两份库。清理默认目录时只清理对应旧工作数据，保留 bootstrap 和必要锁/配置，禁止递归删除整个 `~/.perch`。

默认目录、选择自定义路径、原件复制和上述迁移机制都是后续开发要求；本轮只更新文档，不实际创建该目录或迁移机器上的任何数据。

## 2. 通用字段与约束

- 业务实体使用 UUIDv7 字符串作为主键；不能用标题、数组下标或远端 ID 充当本地主键。
- 事件时间统一使用 UTC Unix 毫秒 `INTEGER`。`created_at_ms`、`updated_at_ms` 必填；本地可编辑实体增加 `revision INTEGER NOT NULL DEFAULT 1`。
- 日期使用 `YYYY-MM-DD`，IANA 时区使用 `Asia/Shanghai` 等名称；Rust 类型负责严格解析。不能用浏览器隐式日期解析推断时区。
- 枚举以稳定英文值存储，数据库 `CHECK` 与 Rust 枚举共同约束；中文仅为 UI 文案。
- JSON 使用带版本结构并在 Rust 反序列化校验；常用筛选、引用和唯一性字段必须独立列出，不能把整个领域对象塞成一个 JSON。
- UUID 关联均设外键；业务关系默认 `ON DELETE RESTRICT`，只有明确由父实体独占的子行允许 `CASCADE`。
- 同一编辑器保存携带 `expected_revision`；`UPDATE ... WHERE id = ? AND revision = ?` 未命中返回冲突，保留草稿并提示刷新，不能静默覆盖。
- 普通归档使用 `archived_at_ms`。首版不为未知同步协议给所有表添加墓碑；真正删除与恢复历史按领域保留规则实现。

## 3. 项目、任务与时间模型

### 3.1 项目与外部对象

| 表 | 关键字段与约束 |
| --- | --- |
| `workspaces` | `id`、`name`、`default_timezone`、`created_at_ms`；默认单条 |
| `projects` | `id`、`name`、`description`、`source(local/zentao)`、`local_status(todo/doing/done/closed)`、`owner_label`、`due_date`、`external_object_id?`、`archived_at_ms`、通用版本字段；外部对象关联唯一 |
| `tasks` | `id`、`title`、`notes`、`source(local/zentao)`、`project_id?`、`external_object_id?`、`local_status`、`priority(low/normal/high)`、`sort_key`、`completed_at_ms?`、`closed_at_ms?`、`close_reason?`、`archived_at_ms?`、通用版本字段 |
| `task_schedules` | `task_id` 主键及外键、`kind(all_day/timed)`、`timezone`、`start_date?`、`end_date_exclusive?`、`start_at_ms?`、`end_at_ms?`；一个任务首版最多一个排期，未排期没有此行 |
| `task_deadlines` | `task_id` 主键及外键、`kind(date/instant)`、`due_date?`、`due_at_ms?`、`timezone`；与个人排期独立 |
| `task_activity` | `id`、`task_id`、`kind`、`before_json`、`after_json`、`operation_id`、`created_at_ms`；用于用户可见状态历史和短期撤销，禁止写入密钥 |
| `mutation_receipts` | `(workspace_id, mutation_id)` 唯一、`command`、`input_hash`、`result_json`、`created_at_ms`、`expires_at_ms`；已提交本地写请求去重，保留期内相同 ID 不同输入返回冲突，结果无凭据 |

外部来源要求 `external_object_id` 非空，本地来源要求为空。本地任务可关联本地或禅道项目，不产生远端写入。有关联任务的项目不能直接删除，先迁移本地任务或归档；禅道任务的项目关系不可由普通本地表单修改。

本地状态与禅道状态是两个字段体系。拖拽、更改详情、勾选完成只写 `tasks.local_status`；完成时记录 `completed_at_ms`，关闭时记录 `closed_at_ms`，重新打开清理当前状态时间并在活动表保留历史。时间段结束不会自动完成任务。项目计数从当前任务派生，不持久化容易漂移的“完成任务数”。

### 3.2 结束时间与跨日

现有预览仅支持“日期 + 同日开始/结束时间”。正式模型支持跨日，实施时同步将详情表单扩为开始日期时间、结束日期时间；同日操作仍作为默认快捷录入。此处是正式实现对预览的明确扩展，不能沿用 `end <= start` 的字符串比较处理跨日数据。

- 带时间排期：`start_at_ms` 必填，`end_at_ms` 可空；有结束时必须 `end_at_ms > start_at_ms`，日期字段为空。不设置结束时保留单个时间点，日历可用最小视觉高度展示，但不能把视觉高度写回为真实结束时间。
- 全天排期：日期字段必填，`end_date_exclusive > start_date`，时间戳字段为空。单日 9 月 7 日保存 `[2026-09-07, 2026-09-08)`，不是在 UTC 中硬减一毫秒。
- 前端选择 IANA 时区，Rust 将输入的本地日期时间转换为 UTC；夏令时不存在的时间拒绝保存，重复时间要求明确偏移。保留排期创建时区，不因电脑切换时区重写绝对时刻。
- 日历带时间事件按查看时区转换后拆成每日可视段，所有段引用同一个任务和排期；全天日期保持日期语义。拖动整段保留真实时长，调整边界才改变时长。
- 日期截止在指定时区的次日零点后才逾期；时间截止比较 UTC 时刻。无截止时间的任务不得用排期结束时间自动推导逾期。

将预览数据导入开发种子时，`date` 加 `time` 按明确种子时区转换；有日期无时间变为全天；缺少日期但有时间进入校验报告。预览示例不是用户生产数据，首次安装默认空库，不自动导入演示会话和示例供应商。

## 4. 禅道缓存与关系一致性

| 表 | 关键字段与约束 |
| --- | --- |
| `zentao_connections` | `id`、`name`、`base_url`、`server_version`、`api_profile`、`account_label`、`credential_ref?`、`management_enabled`、`enabled`、`last_success_at_ms?`；API 能力由实际 22.0 实例验证 |
| `external_objects` | `id`、`connection_id`、`remote_type(project/execution/task)`、`remote_id TEXT`、`parent_object_id?`、`remote_project_id?`、`remote_execution_id?`、`title`、`remote_status`、`remote_begin_date?`、`remote_end_date?`、`remote_updated_at?`、`payload_json`、`payload_hash`、`last_seen_run_id`、`last_synced_at_ms`、`availability(available/inaccessible/unknown)` |
| `sync_runs` | `id`、`connection_id`、`scope_json`、`status`、`started_at_ms`、`finished_at_ms?`、计数、`error_code?`、脱敏 `error_message?` |
| `sync_scope_checkpoints` | `(connection_id, scope_key)` 唯一，`cursor_json?`、`last_complete_run_id?`；完成相应范围后才推进 |
| `remote_operations` | `id`、`connection_id`、`external_object_id`、`action`、`request_hash`、`before_snapshot_json`、`status(prepared/sent/confirmed/failed/unknown)`、`requested_at_ms`、`confirmed_at_ms?`、脱敏错误信息 |

外部唯一键为 `UNIQUE(connection_id, remote_type, remote_id)`。远端 ID 用文本保存，以适配不同实例的编码，不使用 JavaScript Number 转换。项目、执行、任务类型不能混淆：项目无父级，执行归属项目，任务归属执行并解析到项目；若实际实例支持其他层级，由 API 适配器明确处理，不能猜关系。

同步先将完整页写入暂存结果并校验父级；同一批事务先 upsert 项目，再执行、任务，最后更新本地关联。未拿到父级的条目进入等待依赖状态，补拉成功后发布，不绑定到同名项目。超大范围允许按完整关联子集分批事务，只有全范围成功才推进对应 checkpoint；某页失败不得把未出现在已下载页面中的对象标记为删除。

同步仅更新远端缓存、远端展示标题和确定的项目关系；保留本地状态、个人备注、优先级及排期。首次导入才按经验证映射初始化一次本地状态；后续读取绝不再次映射。401/403/404 或范围变化仅记录不可访问/未知，保留已有项目、任务和本地工作。

显式禅道操作独立写 `remote_operations`，提交前展示远端对象和变更，成功后回读远端确认。网络超时的状态为 `unknown`，先回读判定，不能直接重试导致重复动作；客户端请求 UUID 仅在服务器支持时能保证远端幂等，不把本地 UUID 当作服务器已去重的证据。

## 5. 应用、分类和设置

| 表 | 关键字段与约束 |
| --- | --- |
| `app_categories` | `id`、`name`、`normalized_name UNIQUE`、`sort_key`；未分类用 `apps.category_id IS NULL`，无需伪造可删除的系统分类行 |
| `apps` | `id`、`name`、`url`、`description`、`logo_asset_id?`、`category_id?`、`favorite`、`sort_key`、`last_opened_at_ms?`、通用版本字段 |
| `app_settings` | `key` 主键、`value_json`、`schema_version`、`updated_at_ms`；仅保存界面、时区、备份偏好等非敏感设置 |

应用 URL 只接受 `http` / `https`，Rust 再次解析校验。分类规范化采用 Unicode NFKC、trim 和大小写折叠的固定规则并版本化；显示名保留用户输入。改名只改分类行，应用通过 ID 展示新名；删除分类与迁移全部引用在同一个事务内完成。事务失败保留原分类和应用关联。

## 6. 模型、Agent、技能与会话

| 表 | 关键字段与约束 |
| --- | --- |
| `model_providers` | `id`、`name`、`protocol`、`base_url`、`credential_ref?`、`enabled`、通用版本字段 |
| `models` | `id`、`provider_id`、`name`、`remote_model_id`、`capabilities_json`、`enabled`、`current_revision_id`；同一供应商内远端模型标识按能力配置去重 |
| `model_revisions` | `id`、`model_id`、`revision_no`、`protocol`、`base_url`、`remote_model_id`、`capabilities_json`、`parameters_schema_version`、`embedding_dimensions?`、`embedding_revision_label?`、`created_at_ms`；不可变、不包含密钥 |
| `agents` | `id`、`name`、`description`、`icon_key`、`enabled`、`current_revision_id`、通用版本字段 |
| `agent_revisions` | `id`、`agent_id`、`revision_no`、`model_revision_id`、`system_prompt`、`parameters_json`、`created_at_ms`；不可变 |
| `skills` | `id`、`name`、`description`、`enabled`、`source(local/imported)`、`current_revision_id`、通用版本字段 |
| `skill_revisions` | `id`、`skill_id`、`revision_no`、`frontmatter_json`、`body_markdown TEXT`、`content_sha256`、`source_asset_id?`、`created_at_ms`；不可变 |
| `agent_revision_skills` | `(agent_revision_id, skill_revision_id)` 主键、`position`，固定技能顺序 |
| `conversations` | `id`、`title`、`agent_id`、`agent_revision_id`、`selected_knowledge_base_id?`、`created_at_ms`、`updated_at_ms`、`archived_at_ms?` |
| `messages` | `id`、`conversation_id`、`sequence`、`role`、`content_text`、`status(pending/streaming/completed/stopped/failed/interrupted)`、`generation_id?`、`created_at_ms`；会话内 sequence 唯一 |
| `message_attachments` | `id`、`message_id`、`asset_id`、`kind(image)`、`position`、`width`、`height`；真实视觉输入的管理文件引用，纳入备份与删除引用检查 |
| `chat_generations` | `id`、`conversation_id`、`user_message_id`、`assistant_message_id`、`agent_revision_id`、`model_revision_id`、`knowledge_base_id?`、`index_generation_id?`、`request_options_json`、`status`、用量、耗时、脱敏错误；保存此次实际发送的非敏感配置快照 |
| `message_citations` | `id`、`message_id`、`document_revision_id`、`index_generation_id`、`chunk_id?`、`chunk_id_snapshot`、`document_title_snapshot`、`locator_json`、`excerpt_snapshot`、`excerpt_sha256`、`retrieval_score`、`rank` |

`credential_ref` 指向逻辑凭据槽，不把真实 Keychain 查询结果放进 model revision 或聊天快照。供应商编辑或技能编辑产生新 revision，新会话使用新 revision；已有会话保留既有配置。每次生成仍记录实际模型和知识库 generation，避免历史上下文被当前配置重写。供应商停用后阻止新请求；历史文字照常可读。

模型远端标识、协议、地址或 embedding 参数变更时创建新 revision；被知识库使用的旧 revision 保留，切换知识库模型必须显式重建索引。不能静默复用旧向量；供应商不暴露模型版本时标记“版本未固定”，保存配置与维度指纹，并在重新生成向量时建立新 generation。

技能正文的唯一可编辑权威是 `skill_revisions.body_markdown`。导入 MD 文件作为不可变原件保存在 objects，解析得到 frontmatter 和正文；编辑仅更新数据库的新 revision，导出时从数据库生成 MD。首版不监控外部 SKILL.md 双向同步，避免磁盘文件与数据库同时可写形成冲突。YAML/JSON 使用结构化解析、大小和深度限制，导入内容视为用户数据，不执行脚本或任意工具命令。

流式回复按受限频率合并写入，不按每个 token 提交事务；结束/停止/失败立即提交最终状态。异常退出后将悬空 `streaming` 标记为 `interrupted`，保留已提交片段；禁止自动重复发起收费请求。Agent、技能、模型、知识库被历史引用时默认归档或禁用，不级联破坏历史。

## 7. 知识库、文件、分块和向量

### 7.1 实体

| 表 | 关键字段与约束 |
| --- | --- |
| `assets` | `id`、`sha256 UNIQUE`、`storage_key UNIQUE`、`byte_size`、`media_type`、`created_at_ms`；路径由服务端生成，原始文件名不作为路径 |
| `knowledge_bases` | `id`、`name`、`description`、`embedding_model_revision_id`、`active_generation_id?`、`desired_source_revision`、`archived_at_ms?`、通用版本字段 |
| `documents` | `id`、`knowledge_base_id`、`display_name`、`current_revision_id`、`status`、`deleted_at_ms?`、通用版本字段；同库规范化名称唯一，重名上传要求改名或显式新版本 |
| `document_revisions` | `id`、`document_id`、`revision_no`、`original_asset_id`、`source_sha256`、`parser_name`、`parser_version`、`parse_options_json`、`status`、`extracted_text TEXT?`、`text_sha256?`、`error_code?`、`created_at_ms` |
| `document_blocks` | `id`、`document_revision_id`、`ordinal`、`kind`、`text`、`locator_json`；保留页码、幻灯片、工作表/单元格、标题等来源结构 |
| `index_generations` | `id`、`knowledge_base_id`、`source_revision`、`embedding_model_revision_id`、`dimensions`、`distance_metric(cosine)`、`normalization(l2)`、`chunker_version`、`chunk_options_json`、`status(building/ready/active/retired/failed)`、`created_at_ms`、`activated_at_ms?` |
| `index_generation_documents` | `(generation_id, document_revision_id)` 主键；冻结本代完整源文档集合 |
| `chunks` | `id`、`generation_id`、`document_revision_id`、`ordinal`、`text`、`locator_json`、`token_count?`、`content_sha256`；在一个 generation 内顺序稳定 |
| `embeddings` | `chunk_id` 主键及外键、`dimensions`、`vector_f32_le BLOB`、`vector_sha256`；字节长度必须为 dimensions × 4 |
| `background_jobs` | `id`、`kind`、`dedupe_key UNIQUE`、`entity_id`、`input_revision`、`status`、`phase`、`attempts`、`next_run_at_ms`、`lease_owner?`、`lease_until_ms?`、`cancel_requested`、`checkpoint_json`、`progress_json`、脱敏错误 |

首版接收 PPT/PPTX、DOC/DOCX、XLS/XLSX、TXT、MD、PDF，单文件 30 MB，拒绝空文件。扩展名只是第一层检查，还须识别文件头、容器结构、压缩展开总量和解析器超时；宏、链接、嵌入对象不执行。旧二进制 Office 和扫描 PDF 的解析依赖及 OCR 能力按集成文档验收，无法解析时明确失败或待 OCR，绝不标记已索引。

原文件由 `assets` 保存，提取后的可检索正文及来源结构由 SQLite 保存。全文索引、分块与向量都可从原件/提取正文和固定版本配置重建；它们不是唯一原始数据。删除文档首先逻辑移除当前库引用并提高 `desired_source_revision`；旧引用快照仍能展示已删除来源，垃圾回收不得删除历史仍需的资产。

### 7.2 索引发布和检索

1. 创建 generation，冻结文档 revision 集合、embedding model revision、维度和分块配置。
2. 后台任务完成提取、分块和向量化，检查返回向量数、维度、有限数值和非零范数，L2 归一化后保存为小端 float32 BLOB。
3. 完成全量计数、哈希和失败文档校验后，在单个事务内确认 `source_revision == knowledge_bases.desired_source_revision` 且模型配置未变化，再切换 `active_generation_id`。
4. 源数据已改变则本代作废或进入待重建，不能让旧任务迟到后覆盖较新的索引。
5. 切换前旧 active generation 可继续服务；首次建立时显示处理中且不提供虚假检索。删除的文档即使仍存在旧 generation，也必须在检索结果中排除。
6. 生成请求固定一个 generation ID，从该代检索并保存引用；重建时不混用新旧维度。进程内读租约与持久化会话引用共同决定旧代回收时机。

首版默认使用 Rust 对当前知识库 active generation 的向量做精确余弦检索，避免基础功能依赖动态 SQLite 扩展。遍历可流式执行，使用 top-k 堆，复杂度 `O(N × d + N log k)`；不能逐条跨 IPC 返回向量。查询使用该 generation 的 embedding model revision 和相同归一化，维度不符立即拒绝。

全文检索使用 FTS5，正文仍以普通表为权威。FTS 行与源行在同一数据库事务更新或使用经测试的触发器；导入后可完整重建。中文基线使用 Rust 成熟分词器（候选 jieba-rs）对原文和查询同版本分词，词元串进入 FTS5；分词词典变化需重建。不能仅用英文 `unicode61` 测试证明中文检索有效，trigram 可作为另行验证的子串增强。FTS 和向量融合采用可配置 RRF，不直接相加不同量纲的评分。

`sqlite-vec` 只作为后续可选优化，启用前通过 macOS arm64/x86_64、Windows 构建、签名、许可、SQLite ABI、加载和备份恢复测试；它提供的能力与实际性能以锁定版本验证，不能预设一定具备 ANN。扩展不可用时默认路径仍能打开业务库和检索。向量专用索引存独立可重建缓存库，业务库保留标准 BLOB，避免恢复数据库必须依赖扩展加载。

首版性能验收基准暂定单库 10,000 chunks、1,024 维，本地检索 p95 ≤ 1 秒，向量核心计算以 500 ms 为优化目标，测试机规格、冷热缓存和内存占用写入结果。模型网络时间单独统计；超过基准时先测量，再决定增加专用索引，不能宣称支持无限文档量。

## 8. 凭据隔离

macOS 使用系统 Keychain。SQLite 只保存不透明的 `credential_ref` 和“是否已配置/最后验证”的非敏感状态；Windows 后续实现相同 Rust `CredentialStore` 接口并单独验证 Credential Manager。不得将令牌放在 localStorage、日志、数据库、Markdown、NDJSON 或请求快照。

凭据槽由 `(workspace_id, provider_or_connection_id, purpose)` 标识；Keychain service 命名包含应用标识。网络请求在 Rust 内解析槽并读取密钥，仅在请求期间驻留内存。前端编辑密钥后调用专用写入命令，后续只收到掩码状态，不能提供读取原密钥的通用 IPC。

数据库与 Keychain 没有跨系统事务：先创建新凭据槽，数据库事务切换引用，提交后清理旧槽；失败时清理未引用槽并记录可重试清理任务。恢复/迁移到另一机器后所有连接默认要求重新配置凭据，保持业务数据可浏览，不把缺少密钥误报成数据恢复失败。

## 9. SQLite 事务与文件一致性

- 使用嵌入的版本化 SQL 和 migration ledger；rusqlite 使用 bundled SQLite，启动连接设置 `foreign_keys=ON`、`journal_mode=WAL`、`busy_timeout`，持久业务优先 `synchronous=FULL`。每个连接都必须启用外键。
- 单独的写入调度器串行执行写事务，读取使用有界连接池。不要在写事务中等待网络、模型回复或文件解析。
- 高频搜索/日历查询为 `tasks(project_id, local_status)`、状态与归档、排期起止时间、外部唯一键、消息会话序号等增加索引，并通过真实数据 `EXPLAIN QUERY PLAN` 验证。
- FTS5 编译能力在启动诊断和 CI 中验证；不依赖开发机恰好安装的 SQLite 命令行或扩展。
- 同步 upsert、任务状态与活动、分类迁移、消息最终提交、generation 发布、资产引用更新各自具有明确事务边界。

上传文件按“临时写入 → 检查大小/格式 → 哈希和刷盘 → 同文件系统原子重命名至 objects → 数据库事务创建资产及引用”的顺序。若重命名后数据库失败，只产生可回收孤儿文件；不能先提交已就绪引用再写文件。相同内容哈希复用资产；替换文档生成新 revision，不覆盖原文件。

文件 GC 从数据库引用集合计算，不靠可漂移的手写计数器。删除前取得 GC 锁并检查没有备份/恢复读取租约，仅回收经过保留期且无文档版本、Logo、技能原件或其他合法引用的资产。删除流程先把候选记录写入 GC 日志，再删除文件，最后删除资产记录；崩溃后幂等恢复。启动检查数据库引用但文件缺失的条目，显示需要恢复，不能静默删业务记录。

## 10. Schema 迁移与失败恢复

1. 取得工作空间独占锁，读取 schema 版本；版本高于当前 App 支持范围时拒绝写入并提示使用新版，不能自动降级。
2. 执行迁移前建立一致备份，记录旧 schema、目标 schema、迁移脚本校验和。
3. 迁移在事务中运行；需重建大表时预检可用磁盘，使用分阶段迁移和可恢复检查点，不能绕过外键而不补校验。
4. 成功后执行 `foreign_key_check`、必要业务约束验证，并打开正常服务。失败回滚事务或切换回完整旧 generation，保留诊断和原库。
5. 任何已发布 migration 不修改内容，后续修正用新编号。降级通过恢复迁移前备份完成，不依赖未经测试的 down SQL。

数据库 schema 以 `schema_migrations(version, checksum, applied_at_ms)` 为权威；若使用 SQLite `user_version` 作为诊断摘要，只由迁移管理器派生写入，不另建第二套版本判断。CI 至少验证空库建表、每个已发布版本升级到当前版本、迁移中断、磁盘不足和新库被旧 App 打开。

## 11. 一致备份与恢复

### 11.1 完整备份

备份不能直接复制正在运行的 `.sqlite3`，也不能假定复制它和 `-wal` 两个文件就得到一致结果。通过 rusqlite 的 backup 功能调用 SQLite Online Backup API，保持受控备份适配层，不在业务代码混用多套数据库访问方式。

备份流程短暂停止新写入、等待当前事务完成并暂停 GC，生成数据库快照；从快照本身读取资产列表，复制这些不可变对象并逐个验证 SHA-256。快照完成后业务可恢复写入，但 GC 租约一直保留到文件复制完成。整个备份包的资产清单严格来自同一快照。完成前使用 `.partial` 文件，全部验证后原子重命名为最终包；取消或失败不覆盖已有成功备份。

备份可能包含工作文档和聊天正文。导出位置由用户选择，使用操作系统文件权限限制访问；凭据永不包含。首版可做明文便携包并在界面明确说明内容，若提供加密，必须使用经维护的认证加密容器格式和独立密码，不能自行发明加密算法。

### 11.2 恢复

首版“恢复备份”采用整体替换工作空间，不做静默合并。先在 staging 解包校验格式版本、schema 支持范围、条目数量、展开总量、校验和、SQLite 完整性和引用；成功后创建新 generation，进行必要迁移，再持有独占锁原子切换 `current.json`。旧 generation 保留至恢复验证通过与保留期到达。恢复失败保留当前工作空间可用。

恢复压缩包必须拒绝绝对路径、`..`、Windows 盘符/UNC、符号链接、硬链接、设备项、重复规范化路径及大小写冲突；限制文件数、单项大小、压缩比和总展开体积以防 Zip Slip 和解压炸弹。不要把包内文件名直接拼到用户路径上。manifest 中每个路径只能指向受控包内对象，不允许 URL 自动下载。

恢复后执行数据库完整性/外键检查、实体计数/校验和检查、附件读取抽检、索引版本检查，所有后台 job 从可恢复状态重新排队。凭据状态设为待配置，远端管理默认关闭，避免换机后自动发出写请求。首次正常启动后才能清理旧 generation。

## 12. 可迁移逻辑导出格式

除了原生备份，提供与数据库实现无关的逻辑导出。逻辑导出与备份共享一致快照流程，不能一边遍历活库一边接受更改。第一版恢复只支持整体导入到空工作空间或替换；未来合并导入必须另定冲突方案。

```text
workbench-export.zip
  manifest.json
  data/projects.ndjson
  data/tasks.ndjson
  data/task_schedules.ndjson
  data/...                     # 全部有权威业务意义的实体
  objects/sha256/ab/<hash>
  readable/skills/<uuid>.md     # 可选派生阅读副本
  readable/conversations/<uuid>.md
```

下面是格式示例，不是现有实现或真实用户数据：

```json
{
  "format": "workbench-logical-export",
  "format_version": 1,
  "app_version": "0.1.0",
  "source_schema_version": 1,
  "workspace_id": "01992158-0c00-7000-8000-000000000001",
  "export_id": "01992158-0c00-7000-8000-000000000002",
  "created_at": "2026-09-07T02:00:00Z",
  "time_encoding": "unix-ms-utc",
  "includes_secrets": false,
  "includes_derived_indexes": false,
  "files": [
    {"path": "data/tasks.ndjson", "bytes": 0, "rows": 0, "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"}
  ]
}
```

NDJSON 为 UTF-8，每行一个对象，字段保留稳定 ID、实体版本和明确的 null；不能把 `undefined`、本地绝对路径或密钥写入。所有可迁移权威表均列在 manifest，包含源文件、提取正文、模型/Agent/技能 revision、历史会话和引用快照。导出过滤内部诊断、临时任务租约和凭据引用的机器定位信息，改用“需要配置凭据”的逻辑槽描述。

向量、FTS 和可重建缓存默认不进入逻辑导出，导入后重建；引用的原文片段快照必须导出，以便重建前仍能阅读历史证据。MD 文件是阅读副本，逻辑导入只使用 `data/` 和 `objects/`，不能让同一技能同时从 NDJSON 和 MD 导入成两个权威版本。未知必需格式版本明确拒绝，旧格式经有测试的转换器升级。

逻辑包保留历史 `index_generations` 元信息和被引用的 document revisions；`chunks` 可省略。省略分块时导出引用的 `chunk_id=null`，保留 `chunk_id_snapshot` 和片段哈希；该可空外键采用 `ON DELETE SET NULL`，正文证据不依赖可重建行。导入后历史代标记 `retired`，知识库 `active_generation_id` 清空并重建，不能只恢复 active 指针而缺少向量。已有 model/agent/skill revision 当前指针的循环外键采用可空引导插入或可延迟约束，在同一事务提交前填充并验证。

## 13. 未来 PostgreSQL 迁移路线

引入 Postgres 的触发条件是需要服务端账号、跨设备同步、团队权限或集中备份，而不是单机文件增长后立即替换 SQLite。跨设备同步是新增产品能力，复制一个 SQLite 文件到网盘不等于同步方案。

1. 服务端沿用领域 ID、DTO 和外部唯一键，建立认证、权限与服务 API；桌面仍保留 SQLite 离线缓存或明确改为在线客户端。
2. PostgreSQL 中 UUID 字符串转为 `uuid`，UTC 毫秒转 `timestamptz`，日期转 `date`，JSON 转 `jsonb`，BLOB 转 `bytea` 或 pgvector。布尔值、排序规则和大小写唯一性分别验证。
3. 使用逻辑包经过校验导入：先工作空间与连接元信息、资产，再项目和外部对象，随后任务、AI revision、会话及引用；循环当前 revision 指针采用先导入实体壳、再填指针的事务流程。
4. 导入资产至服务端对象存储，保留 SHA-256 与实体 ID；外部连接凭据在服务端独立配置，不上传桌面 Keychain 数据。
5. 按表计数、逐条规范化哈希、关系完整性、时区边界和代表性用户流程核验；失败可丢弃新建目标 workspace 并重试，不修改原始本地导出。
6. 若加入双端持续写入，另行设计服务端变更序列、游标、墓碑、冲突处理、离线 outbox 和账户权限。首版 `revision` 只防同库覆盖，不声称解决多设备冲突。

不要承诺切换连接串即可将 SQLite 升级为 Postgres；新增 PostgreSQL 驱动与 adapter，并适配 DDL、FTS、向量索引、文件存储和权限模型。也不要把禅道同步游标与工作台未来设备同步游标合并。

## 14. 数据验收清单

| 场景 | 通过标准 |
| --- | --- |
| 任务同日/跨日/全天/无结束 | 保存后重启回显一致，跨日多个日历段仍对应一个任务；缺失开始和逆序被拒绝 |
| 时区和夏令时 | 上海、洛杉矶切换不重写时刻；不存在时间拒绝，重复时间明确偏移；全天日期不漂移 |
| 项目与禅道关系 | 相同名称/远端 ID 在不同连接不混淆；任务按远端父级关联；本地拖拽不改变远端状态 |
| 分页同步失败 | 不推进失败范围 checkpoint，不误删未读对象；重试无重复项目或任务 |
| 状态与撤销 | 数据、计数与活动一次提交；冲突时保留草稿，不覆盖较新 revision |
| 技能和 Agent 历史 | 编辑技能/模型后新会话使用新版本，旧消息保留原配置和引用 |
| 知识库重建 | 中途增删文档使过期 generation 无法发布；模型维度改变不会与旧向量混用 |
| 文档删除 | 新检索立即排除删除文档，历史消息仍能查看来源名称与片段 |
| 文件写入崩溃 | 在写入、重命名、DB 提交各阶段注入失败，无已就绪引用指向未落盘文件；孤儿可回收 |
| 一致备份 | 备份期间并发编辑/上传/删除后，恢复得到一个完整快照且附件哈希全部匹配 |
| 恢复失败 | 损坏包、新格式、空间不足、Zip Slip、哈希失败均不替换当前工作空间 |
| 默认存储位置 | 首次明确初始化在用户主目录 `.perch` 建库，设置回显实际完整路径，不使用硬编码用户名 |
| 自定义路径迁移 | SQLite、原件及根内备份迁移后重启仍可读，ID/关系/哈希一致，Keychain 凭据引用不变 |
| 目录迁移失败 | 权限不足、磁盘满、卷离线、复制中断、切换前后崩溃均可恢复，源目录保留，不创建替代空库 |
| 恢复默认与冲突目录 | 非空目标/嵌套路径拒绝自动合并，恢复默认走相同流程，清理不删除启动配置 |
| 换机恢复 | 无密钥泄漏，数据可读；连接要求重配凭据，索引可重建，远端管理默认关闭 |
| Schema 升级 | 每个已发布版本可升级；失败回滚；旧 App 不写新 schema |
| 索引降级 | 无 sqlite-vec 时业务库可开、全文及精确向量检索可用；损坏衍生索引可重建 |
| Postgres 可迁移性 | 逻辑包在空目标导入后 ID、关系、正文、时间和资产哈希保持一致；不要求首版交付服务器 |

以上属于后续实现验收，不代表当前静态预览已通过数据库、备份、解析或迁移验证。
