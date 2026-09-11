# 发布与在线更新

## 当前方案

推送稳定 Tag `vX.Y.Z` 触发 `.github/workflows/release.yml`：校验版本与中文提交，构建 macOS Apple Silicon 和 Windows x64，验证更新包签名，再统一公开 GitHub Release。普通分支提交只执行 CI，不发布安装包。暂不支持预发布 Tag、Intel Mac 和 Linux。

当前目录尚未初始化 Git，也未绑定 GitHub 仓库及正式签名配置。工作流和应用更新链路已实现；真实 Actions、Windows 原生安装与已安装应用在线升级仍待首次发布验证。

更新源使用公开 GitHub 仓库的 `https://github.com/OWNER/REPO/releases/latest/download/latest.json`，由工作流按实际仓库名生成配置，不需要修改源码中的仓库地址。不把 GitHub 私人访问令牌放进客户端。私有仓库需要另行提供公开分发仓库或更新服务，当前工作流会明确拒绝直接发布。

## 首次配置

1. 将当前项目提交到自己的公开 GitHub 仓库并启用 Actions，提交说明使用中文。
2. 在项目目录运行以下命令，将签名密钥保存到仓库以外的安全目录。该命令交互设置密码；不要将私钥或密码发送到聊天或贴进终端日志。

   ```sh
   mkdir -p "$HOME/.tauri"
   npm run tauri -- signer generate -w "$HOME/.tauri/perch-updater.key"
   ```

3. 在 GitHub 仓库 Settings → Secrets and variables → Actions 配置：

   | 类型 | 名称 | 内容 |
   | --- | --- | --- |
   | Variable | `TAURI_UPDATER_PUBLIC_KEY` | 生成的 `.key.pub` 文件内容 |
   | Secret | `TAURI_SIGNING_PRIVATE_KEY` | `.key` 文件内容，不是文件路径 |
   | Secret | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 生成时设置的密码；无密码时可省略 |

4. 妥善备份正式密钥并持续复用。已安装客户端内置公钥，更换签名密钥不能直接让旧客户端信任新版本；密钥丢失需要专门迁移方案或重新手动安装。
5. 初始化 Git 后执行 `npm run dev:hooks` 启用中文提交主题检查。这是纯 Node Git hook，不调用模型，不会增加模型请求。已有自定义 hooksPath 时先合并原有 hook，不直接覆盖其流程。

生成的 `src-tauri/tauri.release.conf.json`、本地密钥和随包运行时均已加入忽略规则；GitHub Secrets 只注入构建步骤。不要提交私钥或把测试密钥用于正式分发。

## 每次发布

以下以 `0.1.1` 为例；首次发布现有版本可使用 `0.1.0`。

```sh
npm run release:prepare -- 0.1.1
npm run release:check
npm run test:release
```

检查并提交本次变更，使用中文主题，例如 `新增：支持应用在线更新`。版本同步覆盖 `package.json`、`package-lock.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 和 `src-tauri/Cargo.lock`。按改动运行相应前端与 Rust 验证后，在干净工作区创建 Tag：

```sh
npm run release:tag
git show v0.1.1 --no-patch
git push origin HEAD
git push origin v0.1.1
```

`release:tag` 仅创建本地 annotated Tag，不自动推送。Tag 注释包含前一祖先稳定版本 Tag 到 HEAD 的所有非 merge 提交，按新增功能、问题修复、体验优化、工程与维护分组；首次发布汇总已有全部非 merge 提交。Release 正文与 `latest.json.notes` 使用同一范围。历史纯英文主题会使校验失败，需在发布前整理尚未发布的提交，不自动重写共享历史。

推送 Tag 就表示启动构建与公开发布。新版本必须高于当前公开版本；不要同时推送多个版本 Tag，GitHub concurrency 只保留有限待执行任务。两个平台可以并行构建，发布步骤串行执行，避免最新清单被旧版本覆盖。

## 安装与更新行为

- 首次从 Release 手动安装 DMG 或 Windows 安装程序。只有内置正式更新源和公钥的版本才能在线更新；当前普通本地开发包必须先手动换成首次发布包。
- 点击左侧品牌旁的版本号，首次展开时检查更新。发现新版本显示橙色标记、最新版本和中文更新内容。支持手动重查，默认无后台轮询。
- 点击「立即更新」通过官方 Tauri Updater 下载、验证签名并安装，显示下载进度；未知下载大小时显示已下载容量。Mac 安装后点击「重启应用」生效；Windows 安装程序会自动退出应用并继续安装。
- 安装前与重启前检查活动聊天；正在运行的会话会阻止操作。其他表单或后台任务没有全局更新锁，更新前应保存当前工作并等待导入等任务完成。下载失败可重试，不伪造成功状态。
- 下载时关闭弹层不会取消下载。更新不会变更 `com.self.workbench` 标识或 `~/.perch` 数据目录；未来数据库升级必须使用版本迁移。
- Tauri 更新签名用于校验更新包，**不等于** macOS Developer ID、公证或 Windows Authenticode。当前工作流未配置这些平台证书，系统可能提示未验证开发者；企业分发需另配平台签名与公证。

## 产物与失败恢复

Release 包含以下实际资产，文件名使用 ASCII 以保证跨平台下载路径稳定：

| 资产 | 用途 |
| --- | --- |
| `Perch_X.Y.Z_aarch64.dmg` | Mac 首次手动安装 |
| `Perch_X.Y.Z_aarch64.app.tar.gz` 与 `.sig` | Mac 在线更新及签名 |
| `Perch_X.Y.Z_x64-setup.exe` 与 `.sig` | Windows 安装及在线更新 |
| `latest.json` | 版本、中文日志、`darwin-aarch64` / `windows-x86_64` 下载地址与签名 |

工作流只在双平台成功后上传到草稿 Release，先验证签名，再上传资产与清单，最后公开。构建失败不会发布残缺版本；上传失败可重跑失败任务，脚本恢复同 Tag 草稿并替换同名草稿资产。已经公开的 Release 和 Tag 禁止覆盖；修复后发布更高版本。

发布脚本会回读公开 `latest.json`，核对版本、两个平台的 URL 与签名。若 GitHub 传播延迟导致回读失败，日志会说明 Release 已公开：单独检查公开地址，不重新覆盖已公开 Tag。

## 验证标准

本地 `npm run test:release` 使用临时 Git 仓库验证多提交汇总，并用临时 Tauri 官方签名器验证签名兼容、错误密钥和产物篡改拒绝。`tests/updater-browser.js` 使用隔离的模拟 IPC 验证更新状态，不能替代真实安装。

首次发布需确认 Actions 两个平台都成功、Release 非草稿且资产完整、公开 `latest.json` 可读取。随后用上一版正式安装包检查新版本，验证真实下载、签名、重启后的版本和已有数据。Windows 原生验收需在 Windows 上进行；没有真实运行结果时，不宣称在线升级已验收。
