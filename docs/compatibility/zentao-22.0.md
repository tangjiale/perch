# 禅道 22.0 认证与同步核验

核验日期：2026-09-08。范围：官方开源源码 Tag `zentaopms_22.0_20260318`；未连接用户的实际实例。服务器插件、统一登录网关和企业定制可能改变行为，需另行确认。

## BUG 同步与状态流转

- BUG 使用 v1：分页读取 `/products`，逐产品读取 `/products/{id}/bugs?status=all&page=…&limit=…`，严格筛选 Token 当前账号的 `assignedTo.account`。不能用“我参与的项目”限制产品范围。
- 详情读取 `GET /bugs/{id}`，编辑使用 `PUT /bugs/{id}`，字段为 `title`、`steps`、`pri`、`severity`、`assignedTo`。
- 解决、关闭、重新激活分别为 `POST /bugs/{id}/resolve`、`POST /bugs/{id}/close`、`POST /bugs/{id}/active`；激活接口路径是 `active`，不能猜测为 `activate` 或 v2 PUT。
- 已修复方案需 `resolvedBuild`；重复方案需 `duplicateBug`。重新激活需 `openedBuild` 数组和指派账号。主干版本标识为 `trunk`。
- API 会将空指派人或特殊账号格式化成 `null`；关闭 BUG 会把指派人设为 `closed`，因此后续个人同步会将其移出。
- BUG 详情可能携带 HTML、图片和 action 历史。前端仅展示安全排版，禁止远端脚本、图片及嵌入请求；缓存仅保存业务字段与冲突核验版本。

源码依据：[v1 路由](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/config/apiv1.php)、[BUG 列表](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/api/v1/entries/bugs.php)、[BUG 详情和编辑](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/api/v1/entries/bug.php)、[解决](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/api/v1/entries/bugresolve.php)、[激活](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/api/v1/entries/bugactive.php)。仅源码及本地模拟验收，未调用真实 BUG 写入接口。

## 浏览器授权结论

所查 API 路由、认证入口和 SSO 模块中，没有发现可直接供第三方桌面应用使用的 OAuth 授权码、PKCE、Device Flow 或浏览器授权后签发 REST Token 的接口。不能仅打开禅道登录页面，就宣称栖点已经获得访问授权。

飞书 SSO 是禅道作为客户端，使用飞书授权结果登录禅道；后台应用集成使用应用代号/共享密钥签名。这两者均不能直接代替 REST Token。网页 Cookie 也不能假定与 API 会话等价。

## 可用官方接口

- `POST <服务根路径>/api.php/v1/tokens`，JSON 字段为 `account`、`password`。
- 正常成功为 HTTP `201`，正文含 `token`。后续 API 使用请求头 `Token`。
- 获取 Token 的端点固定为 v1。22.0 源码有 REST v2 路由，但不能因此改用 `/api.php/v2/tokens`；业务同步的 API 版本与取 Token 入口分开处理。
- 访问权限继承登录账号。密码错误可能触发账号锁定，自动重新登录失败后暂停，直到用户显式重新配置凭据。
- 响应不含 `expires_in`、`refresh_token`。源码 API 会话跳过默认 GC，但部署清理或会话销毁仍可导致失效，不能承诺永久有效。自动恢复通过账号密码重新调用登录接口获取新令牌，不是 refresh_token 续期。
- `authKey` 分支属于既有 IM 认证，不等同于对第三方桌面应用开放的 OAuth 授权流程。

## 栖点实现

“账号登录 / 访问令牌”连接统一通过 `zentao_connect` 保存。账号登录在 Rust 请求官方 Token 接口，Token 写入系统钥匙串，前端不接收 Token 值。默认不记住密码；用户勾选“记住登录凭据，令牌失效后自动重新登录”时，将账号密码与已同意的 HTTP 登录策略存入独立钥匙串条目。SQLite 只保存开启标志和账号名，密码不进入数据库、备份、前端回读或日志。

后续请求复用已有 Token；明确收到 HTTP 401 的读取请求最多触发一次自动登录并重试。403、网络超时、服务器错误不会触发登录；自动登录失败后暂停，需用户重新配置。PUT 修改请求不自动重放，结果不明先同步核对。取消记住选项或改用手动令牌时清除已保存登录凭据；换设备恢复备份后需重新登录启用。

登录请求禁止重定向，20 秒超时，响应上限 64 KB；错误不回显远端正文和凭据。HTTP 发送账号密码需用户在表单中明确勾选，默认不允许；HTTPS 保持系统证书校验。

网络完成后再次验证工作空间、数据代与连接 revision，禁止把迟到结果保存到已切换的目录。已有连接不能更换服务地址，需新建连接以保护项目来源关联。钥匙串保存失败不写连接，数据库保存失败恢复旧凭据；恢复失败明确提示重新登录。

如果要求“系统浏览器登录 → 授权栖点 → 自动返回”，需要禅道管理员提供额外授权桥接：短期一次性授权码、客户端绑定、state/PKCE、严格回调地址和服务端 Token 兑换。不能把长期 REST Token 放入回调 URL，也不能把应用共享密钥内置到桌面安装包。本次没有实现或部署该服务器扩展。

## 项目与执行同步

当前范围仅项目和执行，不查询或导入执行下的具体任务；已移除旧的独立任务搜索及任务项目字段纠正链路。

- 同服务根路径使用 v1 /user 读取 profile.account，不能用连接显示名称推断身份。
- v1 /projects?involved=1&status=all 完整读取当前账号参与项目；包含创建者、PM、项目团队、干系人或白名单成员。v2 involved 查询参数无已验证的等效过滤，因此个人同步固定使用 v1。
- v1 /executions?fields=PM,desc&status=all 完整读取执行目录。只导入相关项目内 PM.account 与当前账号相同的执行，包含 status 为 closed 的执行并映射为本地已关闭。无负责人跳过，负责人字段缺失或无法识别时失败；不能按 realname 或项目负责人筛选。
- 项目 PM 保存到 owner，执行 PM 保存到 remoteExecutionOwner / remoteExecutionOwnerAccount。
- 远端 wait / doing / done / closed 对应本地 todo / doing / done / closed；begin / end 转为全天跨日排期，末日包含当天。
- 官方 22.0 越界页会回到第一页。按 page/total/limit 或 pager 元数据停止，校验页码并保留重复记录检测，不通过忽略重复假装读取完成。
- 完整远端读取成功后，在一个事务中保存项目、执行、同步时间，并清理当前连接下不在本轮范围的旧工作项（包括以前导入的具体任务）；自建任务与其他连接保持。读取、权限、版本校验或写入失败都不会提前删除旧数据。

回归：当前账号身份、参与参数、PM 扩展字段、同名不同账号、包含本人已关闭执行并在重复同步后保留，排除他人执行、请求序列中完全没有 tasks、状态和跨天日期、分页边界与 SQLite 原子清理。仅模拟 HTTP 和临时数据库验证，真实实例仍需安装新版后同步验收。

官方固定版本：zentaopms_22.0_20260318。依据包括 api/v1/entries/user.php、projects.php、executions.php，以及 module/project/tao.php 中参与项目过滤逻辑。

## 从工作台新建执行

新任务选择禅道项目后，由 `save_task` 调用 v1 `POST /executions?project=<远端项目ID>`；已有本地任务改关联不会触发创建。提交前通过 `/user` 确认 Token 账号，读取 `/projects/{id}` 核验项目模型及多执行设置。22.0 的 `executions.php` 不接收 `type`，由项目模型派生；默认短期迭代仅适用于 Scrum／融合敏捷项目。

22.0 `entry::param()` 仅从 `$_GET` 读取，`request()` 读取 JSON 正文。创建入口在 `batchSetPost()` 后用 `param('project', 0)` 再次覆盖所属项目，所以必须同时传查询参数 `project` 与正文 `project`，二者使用同一个已校验远端项目 ID。只传 JSON 会被置为 0 并报“所属项目不能为空”。回归测试必须检查真实 HTTP 请求行与正文的一致性。

字段映射：`title → name`、`notes → desc`、全天 `schedule.start → begin`、`schedule.end - 1 日 → end`；默认 `lifetime=short`、`PM=当前账号`、`teamMembers=[当前账号]`，初始状态 `wait`。代号按稳定任务 UUID 生成；产品、计划为空，可用工作日不自动猜测。

创建前按刚读取的远端项目 begin/end 检查执行日期范围，零结束日期不设上限。创建响应 HTTP 400/422 安全解析字段错误（兼容 message/errors/error 对象及 JSON 编码消息），不显示认证字段、HTML 或完整响应；无法提取时明确说明未取得具体校验原因，不推断为连接失效。days 与产品在 Scrum 默认配置中并非必填，不能凭 400 自动填入或改关联。

POST 提交意图写入现有 settings 表，不存凭据。未知结果不重放，成功响应可用于恢复本地落库；关联项目、负责人、类型、状态及字段全部核验后才返回创建成功。随后的同步按连接、远端类型和远端 ID 复用同一记录。未使用真实禅道验证创建权限或定制字段。

官方依据：[创建入口](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/api/v1/entries/executions.php)、[执行默认字段处理](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/module/execution/zen.php)。

## 执行修改回写

统一 `save_task` 入口支持本地保存和禅道执行回写。官方固定版本 `api/v1/entries/execution.php` 的 `PUT /api.php/v1/executions/{id}` 明确允许 `name`、`desc`、`begin`、`end`、`status`；未提交字段由旧执行补齐。因此仅发送用户改动字段，不提交整个执行对象，也不请求具体任务接口。

- 任务名称对应 `name`，备注对应执行描述 `desc`，不追加动态评论。
- 状态 `todo/doing/done/closed` 对应 `wait/doing/done/closed`。
- 排期只支持全天日期，提交时将本地排他结束日减一天，作为禅道包含末日的 `end`；修改排期缺少首尾日期或使用定时时间会拒绝，不静默丢弃小时分钟。
- 写前读取当前账号和执行详情，核验负责人、项目关联及待修改字段的基线。与拉取同步共用连接锁，发送前使用回滚事务验证本地 revision 和约束。
- 远端成功且返回对象与变更一致后才写本地；超时、响应不完整、远端已变更或本地落库失败会提示同步核对，不自动重发 PUT。
- 新导入执行描述映射备注；旧版非空个人备注保留到用户主动修改，避免升级拉取时覆盖。修改备注后进入远端描述同步管理。

实现和回归使用模拟 HTTP、临时数据库及官方源码核验；未在用户的真实禅道实例上执行写操作。权限、插件及定制网关的兼容性仍需实际使用验收。

## 证据与验收

- [官方 22.0 Tag](https://github.com/easysoft/zentaopms/tree/zentaopms_22.0_20260318)
- [Token 认证实现](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/api/v1/entries/tokens.php#L20)
- [执行读取与修改实现](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/api/v1/entries/execution.php)
- [REST v2 路由](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/config/apiv2.php)
- [API 会话处理](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/framework/base/router.class.php#L1230)
- [API 会话回收规则](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/framework/base/router.class.php#L3915)
- [应用集成签名](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/module/common/model.php#L1834)
- [飞书 SSO 方向](https://github.com/easysoft/zentaopms/blob/zentaopms_22.0_20260318/module/sso/zen.php#L42)

本地使用临时工作空间、模拟钥匙串和本地 HTTP 服务覆盖正确端点、成功响应、错误脱敏、重定向拒绝、超量响应、地址/版本冲突与凭据回滚。浏览器测试使用模拟 IPC，不是实际禅道登录验收。实际实例的版本、登录策略、权限与项目同步仍需用户在桌面应用中验证，不在聊天中提供密码或 Token。


## 执行描述富文本与文件

依据官方源码 tag `zentaopms_22.0_20260318`：`api/v1/entries/file.php` 的 GET 调用文件下载，路径为 `GET /api.php/v1/files/{id}`；`api/v1/entries/files.php` 的 POST 调用 `file.ajaxUpload`，表单字段为 `imgFile`，返回 `id` 与 `url`。均复用 Token 认证。描述中的 `{ID.png}` 是图片引用，不能直接作为浏览器 URL；也支持同源 `index.php?m=file&f=read&t=png&fileID=ID` 与 `file-read-ID.png`。

原生读取先核对当前账号与执行的项目／负责人及远端描述引用，再通过固定文件 API 下载。上传先核验远端版本、执行负责人和项目关联，不自动重试 POST。上传返回的编号与 URL 必须一致；前端存储原始引用而不是预览 data URL，描述更新继续走原 `save_task` 冲突与回写逻辑。
