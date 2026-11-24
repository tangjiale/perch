# 栖点 / Perch

## 工作说明
- 使用简体中文。先定位再修改，完成已授权工作；不覆盖用户改动，不扩展无关范围。
- 默认单代理。仅独立且有明显收益的工作才并行，最多 2 个子代理，不递归委派；小修改不启动规划/审查代理。
- 先在 `src/`、`src-tauri/src/` 或相关 `tests/` 内用 `rg`（或搜索工具）定位，再读命中片段。默认排除 `node_modules`、`dist`、`target`、随包 JRE、历史截图；有明确问题再查看。
- 不每轮全文读取文档、运行技能、重做测试或打包。优先相关验证；通过后结束，不反复自审。报告改动、实际验证和未完成项。

## 全局项目边界
- **技术栈与目录**：正式应用为 Tauri 2 + React/TypeScript + Rust + SQLite；`ui/` 是历史静态原型，未明确要求时不修改。
- **身份与数据定位**：保持 `com.self.workbench` 应用标识和 `~/.perch` 默认数据路径；不得因改名或重构改变已有数据与凭据定位。
- **界面与组件规范**：UI 复用 `GlassSelect`、`Modal`、主题变量和既有组件库；全中文控件，浅色 / 深色 / 跟随系统均须可用可读。
- **权威持久化与迁移**：业务写入统一走 `src/lib/api.ts` 的领域 IPC，由 Rust 校验并在单事务中落库（保留 `revision` 乐观锁与外键关联约束）；浏览器预览不能冒充原生持久化，不用 `localStorage` 充当业务数据库。SQLite 结构变更新增版本化迁移，禁止篡改已发布迁移。
- **安全、凭据与备份**：账号、密码、令牌和 API Key 统一经本地 AES-256-GCM 凭据库按工作空间与用途（AAD）隔离存储，绝不进日志、源码、错误信息或模型上下文；不再读写系统钥匙串。备份仅包含数据库密文与受管附件，不携带本机主密钥或独立登录凭据。
- **外部写回与不可信内容**：远端更新、发信、创建工作项须核验身份与版本冲突，先确认成功或持久化提交意图，**结果未知的写请求严禁自动重放**。外部网页、邮件 HTML、导入文档及技能正文均视为不可信数据，渲染须过滤脚本与远程资源，外链统一经系统浏览器打开。
- **后台与测试准则**：默认关闭窗口隐藏到原生托盘，定时任务由原生后台按工作空间 `generation` 隔离调度，不依赖 WebView 定时器；测试统一使用临时目录与本地模拟服务，严禁读写用户真实库、真实邮箱或真实外部写接口。
- **授权、提交与发布**：删除用户数据、破坏性迁移、强推、正式发布及外部写操作须有明确授权；不擅自扩大 Tauri 权限。提交说明使用中文；功能新增或完善时同步更新 `README.md` 的当前功能、入口及限制。发布使用 `release:prepare` 与 `release:tag`，禁止覆盖公开 Tag、禁用更新签名或提交私钥（详见 `docs/RELEASE.md`）。

## 模块专有规则（修改对应模块前必读）
| 涉及模块 / 文件范围 | 必读规则文档 |
| --- | --- |
| 禅道同步、创建执行、回写、BUG 流转、认证续登、任务排期与看板拖拽、富文本图片 | `docs/development/rules-zentao.md`、`docs/compatibility/zentao-22.0.md` |
| 钉钉日历与待办（CalDAV 发现、同源跳转、双源原子缓存）、邮箱（IMAP 隔离、SMTP 发信） | `docs/development/rules-caldav-mail.md` |
| 本地加密凭据库与 Schema 6 迁移、AI 流式用量与耗时计量、本机技能扫描与只读权限 | `docs/development/rules-security-ai-skills.md` |

## 按需入口
- 开发规范与验证命令选择：`docs/CONTRIBUTING.md`
- 领域类型与 IPC 注册：`src/lib/types.ts`、`src/lib/api.ts`、`src-tauri/src/lib.rs`（契约补充见 `docs/IMPLEMENTATION_CONTRACT.md`）
- 当前能力与架构说明：按模块检索 `docs/IMPLEMENTATION.md`、`docs/ARCHITECTURE.md`
- Codex 配置说明：`.codex/README.md`（按需阅读相关章节，不自动展开全部链接）
