# 实现与验证记录

日期：2026-09-07。版本：0.1.0。目标：macOS 13+ Apple Silicon。

当前为可运行、可安装的开发版本。Tauri、React、SQLite、文件解析及网络适配器已落地；原架构文档的所有工程阶段尚未全量验收，因此不标记为完整正式版。

## 2026-09-08 外部网页打开权限

- 截图中的 `Not allowed to open url` 对应 opener 的 URL scope 拒绝；此前仅声明命令权限，未配置可打开地址。补齐 `opener:allow-open-url` 的 HTTP/HTTPS scope，支持禅道内网 IP/端口、应用网站和 Release 日志。
- 保持仅默认浏览器，不启用本地文件 URL、mailto/tel 或任意程序。权限随安装包生效，已运行旧进程需要完全退出再打开新包。此修复不代表禅道网页 SSO 或 REST 账号认证已实测通过。
- 验证：构建生成的 capability 含 HTTP/HTTPS scope；使用 opener 实际依赖的 glob 匹配库，4 个 HTTP/HTTPS（含内网端口、子路径和 IPv6）地址通过，7 种其他协议被拒绝。截图服务地址只读探测返回 HTTP 302，指向同源 `/index.php`；没有提交账号密码。Mac App/DMG 构建通过。

## 2026-09-08 统计菜单

- 新增 `/#/statistics` 和左侧统计入口，使用按需加载的 Apache ECharts 6。工作与项目、知识与应用、AI 与连接三个视图合计 15 张图表，包含环形饼图、水平/堆叠柱状图、折线图和日历热力图；支持数据表切换、趋势范围、图例、滚动和主题适配。
- `statistics.ts` 从现有工作空间快照聚合，MCP 配置单独走只读 `mcp_list`；未增加数据库表或数据写入。任务类型补齐后端已有的 `completedAt`，不回填历史时间。完成率、日期、重开任务、缺失关联、存量与趋势等口径见 README。
- README 补齐正式应用功能入口及禅道账号连接说明，AGENTS 与开发规范记录后续功能变更必须同步 README。
- 验证：18 项前端测试（含 9 项统计聚合）、10 项隔离浏览器检查通过；覆盖 15 张图表非空像素、图表/数据表切换、日期范围、空态与主题。Mac App/DMG 构建通过。ECharts 所在统计路由块约 678 KB（gzip 230 KB），触发 Vite 500 KB 提示，已保持路由按需加载，未提高阈值掩盖提示。

## 2026-09-08 禅道账号连接

- 核验官方 `zentaopms_22.0_20260318` 认证源码；有 REST v2 路由，但获取 Token 使用固定 `POST /api.php/v1/tokens`。未发现给第三方桌面应用签发 OAuth 授权码/PKCE 的内置流程，飞书 SSO 与应用集成不能等同 REST 授权。
- 新增账号登录/访问令牌两种连接方式，账号登录自动获取 Token 并保存连接；密码不落库，令牌只进系统钥匙串，不返回前端。保留手动 Token 和已有连接重新登录，统一通过 `zentao_connect`。
- 登录无自动重试、禁止重定向、20 秒超时和 64 KB 响应限制，HTTP 登录须明确勾选。保存校验工作空间、数据代和 revision；数据库失败恢复旧凭据，已有连接更换服务地址需新建连接。
- 4 项 Rust 认证测试、12 项模拟 IPC 弹窗交互与 Clippy 通过，覆盖子路径、业务 v2 选择、失败脱敏、钥匙串故障、回滚、防重复提交、HTTP 确认、刷新重试与清除临时凭据。浅色 1440px / 深色 900px 检查，Mac 包重新构建；未使用真实账号或实例完成登录。
- 真正的系统浏览器授权回调需要禅道侧额外适配，本次未实现服务器插件。详细证据与边界见 [22.0 认证核验](compatibility/zentao-22.0.md)。

## 2026-09-08 Tag 发布与在线更新

- 新增 GitHub CI 和 Tag Release 工作流。稳定 `vX.Y.Z` Tag 校验版本与中文提交，构建 Mac Apple Silicon / Windows x64，在双平台成功后验证签名、上传草稿资产、生成 `latest.json` 并公开发布；串行发布防止更新源降级。
- 侧边版本号改为玻璃弹层，接入官方 Tauri Updater 的检查、下载进度、签名验证安装和重启。包含最新版本、更新标记、中文日志、失败重试、浏览器与未配置构建提示。Mac 安装后手动重启，Windows 安装程序自动接管；活动聊天阻止安装和重启。
- `release:prepare` 同步五处版本元数据，`release:tag` 生成汇总多个 commit 的中文 annotated Tag。中文提交 hook 按需启用，不调用模型。签名密钥和生成配置不入库，更新配置在 CI 按实际仓库注入；普通本地构建保留合法空配置。
- 新增带固定版本和 SHA-256 的 Tika / JRE 准备脚本，支持 Mac ARM 和 Windows x64，并保留许可文件。Windows Java 路径适配为 `jre/bin/java.exe`；Windows 原生运行尚待 CI 和目标机验证。
- 已验证：前端 9 项、发布脚本 7 项、Rust 26 项、模拟 IPC 更新交互 12 项；Clippy 无警告、工作流 YAML 和文档链接检查通过。官方 Tauri 临时签名与发布校验器兼容，PDF/DOCX/XLSX/PPTX 解析样本通过。浅色 1440px、深色 900px 弹层已检查，Mac App/DMG 已重新构建。
- 尚未配置 Git 仓库、正式更新公钥和 GitHub Secrets，未推送 Tag、运行真实 Actions 或进行已安装应用升级。首次使用需要安装带正式更新源的 Release 包；平台签名与公证另行配置。操作说明见 [发布与在线更新](RELEASE.md)。

## 2026-09-07 日历、玻璃材质与 Logo 更新

- 新增独立 `src/features/Calendar.tsx` 和 `calendar.css`，恢复原日历稿的小月历、来源选择、未排期区及主日历布局。自绘 Lucide 导航与分段视图切换，移除 FullCalendar 默认字体图标依赖。
- 修复选中态文字对比度、中文全天标签、日/周视图全天区高度；默认月视图，时间视图定位 08:00。排期与事件拖动继续由 FullCalendar 和原 SQLite 保存接口处理。
- `src/glass.css` 统一浅色液态玻璃材质，覆盖侧栏、顶栏、控件、卡片、弹窗和聊天输入。此为 iOS 26 Liquid Glass 启发的 WebView 材质实现，不是 Apple 原生材质 API。
- `public/workbench-logo.svg` 为新 Logo 源图；同步生成 favicon、ICNS、ICO、PNG，应用包内 ICNS 与生成源的 SHA256 一致。
- `tests/calendar-browser.js` 在独立浏览器预览中注入带“界面测试”标记的临时任务，14 项断言通过。测试数据未写入本地数据库；关闭浏览器会清除。
- 5 项排期测试通过，前端构建通过；10 个页面路由无横向溢出。新 `.app`/`.dmg` 生成成功，原生窗口验证月历布局；四视图切换由浏览器交互测试覆盖。
- JRE 只读许可文件导致重复构建无法覆盖缓存，已在 `src-tauri/build.rs` 对本工程生成的解析器缓存处理写权限；不修改原始依赖文件或用户数据。
- 实际原生截图：[月历](screenshots/glass-native-calendar.png)。带临时任务的浏览器截图：[月历](screenshots/glass-calendar-month.png)、[日视图](screenshots/glass-calendar-day.png)、[窄屏](screenshots/glass-calendar-compact.png)。

## 2026-09-08 设置导航与 MCP 配置

- Agent、知识库、技能归入设置 Tab，聊天保留独立入口。旧 `/agents`、`/knowledge`、`/skills` 地址重定向到对应设置 Tab。
- 设置包含禅道连接、大模型管理、Agent、知识库、技能、MCP、数据与备份、关于；URL 的 `tab` 参数保留当前选择，支持方向键、Home、End 切换。
- `src/features/McpSettings.tsx` 提供连接新增、编辑、删除与启停配置；支持 Streamable HTTP 的服务地址和请求头，以及 STDIO 的命令、逐行参数和环境变量。
- `src-tauri/src/mcp.rs` 提供 `mcp_list`、`mcp_save`、`mcp_delete`。普通配置存入当前 SQLite 的 `settings`，键为 `mcp.connection.<id>`；使用 revision 检测并发修改，无新增数据库迁移。
- 请求头和环境变量独立存入系统钥匙串，账号按工作空间和连接 ID 隔离。前端仅读取是否已配置凭据，编辑默认保留，可明确替换或清除。备份不包含钥匙串，换机后需重新配置认证。
- 本次为连接配置管理：保存不会启动 STDIO 进程或访问远端 MCP；尚未实现连接检测、工具发现、Agent 绑定及聊天工具执行。启用状态仅表示配置开关。
- 验证：17 项浏览器断言通过；1440×1000 页面及 900×800 弹框截图检查通过；5 项前端排期测试、2 项 MCP Rust 测试、Clippy 与生产构建通过；Mac `.app` 和 `.dmg` 重新生成。MCP 测试覆盖真实 SQLite 持久化与冲突，凭据测试使用内存适配器，不代表真实钥匙串联调完成。

## 2026-09-08 全局下拉与栖点命名

- 新增 `src/components/GlassSelect.tsx` 与 `glass-select.css`，基于 Radix Select 替换正式应用 16 处可见原生下拉，覆盖聊天、任务、项目、应用分类、Agent 模型、知识库向量模型、供应商协议、模型能力、禅道 API 版本和 MCP 传输方式。
- 表单内部仍由 Radix 提供隐藏原生控件以保留表单语义；用户操作的是自定义玻璃按钮和菜单。菜单挂载在最近的原生 dialog 内，保留空值、禁用、必填与键盘行为。
- 显示名称更新为栖点 · Perch，安装产物为 `栖点.app`、`栖点_0.1.0_aarch64.dmg`。应用 identifier、二进制名、钥匙串 service、SQLite 与默认数据目录不变。
- 回归入口：`tests/glass-select-browser.js` 覆盖空值、菜单选择、弹窗菜单层级和页面溢出；`tests/settings-browser.js` 已切换为操作自定义 MCP 下拉。浏览器检查不替代原生 WKWebView 的真实数据保存验收。
- 验证结果：26 项选择器检查、17 项设置检查、5 项排期测试通过；生产构建与 Mac 双产物打包通过。另实测鼠标选择空值、MCP 传输切换和任务筛选键盘 End/Enter。Impeccable 机械检查未报告问题。截图：[聊天](screenshots/perch-chat-select.png)、[900px 弹框菜单](screenshots/perch-mcp-select.png)。

## 2026-09-08 通用设置与外观

- 设置新增第九个 Tab「通用设置」，位于「关于」前。`GeneralSettings.tsx` 提供浅色、深色、跟随系统和会话字号设置。
- `appearance.ts` 在 React 渲染前加载偏好，监听系统配色与 storage 事件，统一设置根节点 `data-theme`、`color-scheme`、`--chat-font-size`。默认 system / 14px，字号限制 12–24px；非法存储恢复默认，保存失败不发布新状态。
- 外观属于设备偏好，保存在本机 WebView 的 localStorage，键为 `perch.appearance.v1`；不写入业务 SQLite，也不随业务备份迁移。浏览器预览使用自身隔离存储。业务数据路径仍为 `~/.perch`。
- 桌面调用窗口 `setTheme`，能力授权仅增加 `core:window:allow-set-theme`；跟随系统使用 null 交还系统控制，避免将当前颜色固定成主题。原生窗口能力本轮未做完整 WKWebView 交互验收。
- `theme.css` 覆盖全局业务表面、语义状态、FullCalendar 与 Portal 菜单，并兼顾减少透明度、高对比。会话代码块和表格字号使用相对单位，输入框保持原字号。
- 验证：4 项外观单元测试及原 5 项排期测试通过，27 项外观浏览器检查通过；另实测系统深浅切换、刷新保留偏好、字号输入。1440×1000、900×800 截图通过，TypeScript / Vite 构建通过。浏览器测试任务与会话仅存在于测试会话内，不写入真实数据库。
- 截图：[深色通用设置](screenshots/general-settings-dark.png)、[900px 窗口](screenshots/general-settings-dark-compact.png)、[深色日历](screenshots/calendar-dark.png)。

## 2026-09-08 应用图标获取

- `AppLogoPicker.tsx` 使用中文「选择图片」按钮、文件名和预览，隐藏浏览器默认英文文件输入。手动上传支持 PNG/JPEG/WebP，限制 2MB，解码成功后才更新草稿。
- 有效 HTTP/HTTPS 网址稳定 800ms 后自动调用 `website_icon_fetch`，支持手动重新获取。手动上传或切换网址会使旧结果失效；已有保存图标默认保留，点击重新获取可替换。
- Rust `website_icon.rs` 使用 HTML 解析器读取 icon/apple-touch-icon 声明和相对路径，最多尝试三个声明图标，最后回退 `/favicon.ico`。支持 PNG/JPEG/WebP/GIF/ICO，按文件魔数判定，不把 HTML 当图片；SVG 和跨来源图标当前需手动上传替代图片。
- 直接访问目标网站，不使用第三方图标服务、不发送 Cookie 或认证头。仅允许同来源请求及同主机 HTTP 升 HTTPS；页面512KB、图标2MB、整个获取15秒上限，流式读取也限制大小。
- 图标以 data URL 随应用记录保存到 SQLite，后续无需每次加载时访问网站，随现有数据备份迁移。自动获取仅在桌面后端运行，浏览器预览仍可验证手动选图。
- 五项本地 HTTP 后端测试通过，覆盖声明图标、相对路径、错误格式、超限回退、无 Content-Length、跨源跳转及失败。浏览器已验证中文入口、手动 PNG 上传、文件名和实际图片解码；未使用用户网站做原生端到端验收。
- `tests/logo-picker-browser.js` 在隔离 iframe 中使用模拟 IPC，七项交互断言通过，覆盖自动调用、成功预览、网址变化、手动上传优先和失败回退。9 项前端单元测试与生产构建通过，Mac `.app` 和 `.dmg` 已生成。

## 1. 产物与启动

- Mac App：`src-tauri/target/release/bundle/macos/栖点.app`
- 安装镜像：`src-tauri/target/release/bundle/dmg/栖点_0.1.0_aarch64.dmg`
- App 约 221 MB，DMG 约 100 MB，包含 Tika 与 Apple Silicon JRE。
- `npm run tauri dev` 启动桌面开发窗口；`npm run dev` 仅为浏览器页面预览。
- 默认地址 `http://127.0.0.1:1420/`。浏览器明确标记预览，禁止伪装桌面写入。
- 原 `ui/` 原型与图稿保留，正式 React 工程在 `src/`。
- 当前采用 npm 与 package-lock.json；Cargo.lock 锁定 Rust 依赖。相较方案中的 pnpm，此处以实际已安装工具链为准。

## 2. 已实现行为

| 模块 | 当前实现 |
| --- | --- |
| 桌面外壳 | 原生窗口、单实例、版本、侧边栏、路由按需加载、空工作空间 |
| 总览与任务 | 四状态概览、今日及跨日安排、任务增改删、个人状态、优先级、项目筛选、看板/列表、同列排序、跨列拖动、键盘拖动及撤销 |
| 排期与日历 | 开始/结束、全天排他结束日期、时区、截止日期、月/周/日/列表、拖动改期；拒绝夏令时不存在或有歧义的时刻 |
| 项目 | 本地项目创建/编辑/状态/空项目删除、来源显示、计数与关联任务入口 |
| 应用 | 名称、网址、Logo、描述、四列卡片、收藏、分类创建/改名/删除；被引用分类受数据库保护 |
| 模型与 Agent | 供应商、模型与能力配置、Keychain 凭据、目录/模型实际请求测试、多 Agent、参数/提示词/技能、复制/启停/删除保护 |
| 技能 | Markdown/YAML frontmatter/JSON 导入、创建、编辑、启停、关联 Agent、不可变修订历史 |
| 聊天 | 三种协议的真实 HTTP/SSE、历史持久化、固定 Agent/模型/技能快照、停止、部分回复保留、断流失败、重命名、删除、复制、引用片段 |
| 禅道读取 | 按 v1/v2 分页约定读取项目/执行/任务；全量抓取后事务发布；独立个人状态；远端项目关联随同步更新；重复页/父关系异常拒绝发布 |
| 知识库 | 多库、选择向量模型、原件上传/保存/导出、解析重试、分块、Embedding、精确余弦检索与引用；指纹校验、候选结果事务发布 |
| 存储 | SQLite WAL/FULL、关系约束、版本冲突、不可变修订记录、工作空间锁、SHA256 附件清单、ZIP 备份/恢复、目录复制校验切换、保留源目录 |

技能是提示词配置，不自动执行导入文件中的代码或系统命令。新会话固定配置快照；历史会话不会随技能编辑静默改变。

## 3. 数据与安全

默认目录严格使用 `~/.perch`。固定 `bootstrap.json` 保存实际 dataRoot；工作空间定位及业务文件在所选目录。SQLite 是业务权威来源，原件按 SHA256 管理。

禅道令牌和模型密钥只写入系统钥匙串；普通保存拒绝顶层凭据字段。备份不包含系统钥匙串。跨电脑恢复后需要重新输入凭据。

迁移采用复制、校验、切换，源目录保留。备份使用 SQLite OnlineBackup、附件哈希清单和原子发布，不覆盖已有目标文件。恢复只接受空目标目录，拒绝目录穿越和超量包。数据表变更需继续增加版本化 migration；不能直接修改已发布 migration。

解析限制：单个文件 30 MB，解析并发 1，Java 堆 512 MB，60 秒超时，正文 16 MB 上限。关闭外部解析器与 OCR；扫描 PDF 无正文时显示需要文字识别。API 请求拒绝重定向，错误不回显远端凭据正文。

## 4. 已执行验证

| 验证 | 结果 |
| --- | --- |
| `npm test` | 5 项通过：全天、跨日、缺开始、倒置/无效时区、夏令时 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | 18 项通过：磁盘存储、事务、关系、冲突、备份/恢复/迁移、会话重启、禅道 loopback HTTP、Embedding loopback HTTP、SSE、文档解析 |
| `npm run build` | TypeScript 与 Vite 通过，路由拆包后无超 500 KB 的产物块 |
| `cargo clippy --lib -- -D warnings` | 通过，无警告 |
| `npm run tauri build -- --bundles app,dmg` | Apple Silicon `.app` 与 `.dmg` 生成成功 |
| 调试版 WKWebView | 原生窗口实测任务创建、编辑，SQLite 读回对应记录与 revision |
| 发布版 WKWebView | 退出调试版后打开 `.app`，读取之前的任务；通过原生 UI 删除测试记录，数据库计数归零 |
| 随包解析运行时 | 对 `.app/Contents/Resources/resources/parser` 再执行 4 项解析测试，PDF、DOCX、XLSX、PPTX 全通过 |
| 浏览器界面 | Chromium 检查 10 个路由与空状态，390px 宽度无页面横向溢出，桌面及窄屏截图；此项不充当桌面 IPC 验收 |

发布工具链修复：本机 Homebrew Rust 1.96.0 的 release 过程出现 Mach-O `mis-aligned LINKEDIT string pool`。明确将 release/build-override 的 strip 设置为 none 后成功构建。签名检查显示 linker ad-hoc，无 Developer ID、TeamIdentifier 或公证，不能描述为已签名正式分发包。

## 5. 尚未完成的目标与验收

以下均保留为后续工程范围，不能仅因存在页面或适配器就视为完成：

- 真实禅道开源版 22.0 的实例兼容性、权限与写操作回读。当前远端管理入口打开禅道，prepare 返回不支持，confirm 不执行远端写请求。
- 真实模型供应商的三协议聊天、认证、停止、计费 usage 与真实 RAG 全流程联调；当前测试为本地 HTTP/SSE 契约，没有使用用户真实凭据。
- 向量目前在文档 JSON 中，未迁移为设计中的专用 BLOB/FTS 表；全局快照会读取这些数据，不适合大量文档。需完成索引独立存储、分页查询与中文混合检索。
- 后台作业的持久队列、索引取消/断点续跑、token 预算与截断策略；目前重启将流式消息标为 interrupted，切换聊天页面取消生成。
- 完整的启动恢复界面、迁移崩溃日志与自动回滚、逻辑导入导出、历史数据代迁移。当前损坏定位会拒绝启动，失败恢复可能留下未激活的目标目录。
- 设计中的 Rust DTO 自动生成、prepare/confirm 文件授权句柄和专用模型/技能修订表尚未完全对齐；目前使用明确领域命令、版本校验和通用 entity_history。
- 分类删除时迁移所属应用、应用拖动排序、完整技能导入预览、统一确认弹框及全部原生交互用例，尚需继续完善。
- DOC/XLS/PPT 旧格式已接入 Tika，但尚未使用各自的真实样本验收；扫描 PDF OCR 不属于当前解析能力。
- Intel Mac 支持、Windows 真实构建与解析验收、Developer ID 签名及公证、首次 GitHub 发布和原生在线升级验收。Windows 打包路径与自动更新代码已实现，不能替代目标平台验证。

上述本地工程项和真实服务验收项分别追踪；完整功能版本仍应按 DEVELOPMENT.md 的 R01-R16 和 P0-P8 验收。

## 6. 核心入口

- `src/main.tsx`：应用外壳、数据查询、路由。
- `src/features/Work.tsx`：任务、项目与日历。
- `src/features/Resources.tsx`：应用、技能、Agent、知识库。
- `src/features/Settings.tsx` / `Chat.tsx`：连接设置与聊天。
- `src-tauri/src/storage.rs`：SQLite、迁移、备份与修订控制。
- `src-tauri/src/integrations.rs` / `streaming.rs`：模型请求与 SSE。
- `src-tauri/src/zentao.rs`：禅道分页适配器。
- `src-tauri/src/knowledge.rs` / `parser.rs`：原件、解析、向量索引。
- `src-tauri/resources/parser/DEPENDENCIES.md`：随包依赖来源、校验与许可位置。
