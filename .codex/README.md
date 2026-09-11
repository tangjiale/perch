# 项目 Codex 配置

此目录只配置当前项目，不修改 `~/.codex`。从项目根目录启动新任务；已运行任务不保证热加载这些设置。

## 载入方式

- 根 `AGENTS.md` 是短的常驻规则；`docs/CONTRIBUTING.md` 是按需阅读的开发规范。
- `config.toml` 指定当前使用的 `gpt-6-astra`，不覆盖推理强度、服务档位、模型输出详细度、认证、沙箱或审批策略。
- `rules/safety.rules` 对列出的破坏性/发布命令请求确认，不对普通读取、测试、构建增加确认。
- 项目级配置和执行规则需要 Codex 信任该目录才会加载。当前目录尚未初始化 Git，因此增加 `.codex` 根标记；不要为了让配置生效擅自新建仓库或改全局信任列表。
- 显式启动参数、桌面任务模型选择或宿主管理策略可能优先于此配置。当前会话的模型不会因为写入文件而自动切换。

## 控制上下文开销

- 常驻只放必要规则，不把历史实现记录、架构文档或截图说明注入每轮上下文。
- 每次工具输出的历史保留预算为 3000 tokens；这不是总 token、实际命令输出或计费的硬上限。被截断时按行范围读取，不忽略错误或重跑全部日志。
- 默认单代理，配置并发上限为 2；`max_depth = 1` 仅约束支持此字段的 V1 后端。V2 的递归限制依靠项目规则，宿主配置也可能覆盖并发上限。
- 不新增自动 hooks、MCP 服务、角色长提示词、后台任务或会额外发起模型请求的检查器。
- 不设置过小的文档截断阈值或压缩阈值来强行省 token；避免因此漏掉规则或反复压缩。

任何新增常驻说明都有非零 token 开销。本配置只减少可避免的上下文和重复工作，不承诺固定节省比例，也不改变 GPT-6 Astra 的账户额度或计费。

## 执行规则边界

`prefix_rule` 匹配命令词前缀，不是完整的安全系统。参数换序、其他工具、脚本封装、网络或数据库操作可能不在示例规则覆盖内；必须继续遵守 AGENTS 的授权和数据保护要求。不得宣称这些规则能阻断所有危险操作。

已有授权应随实际请求处理，不重复向用户确认。若宿主自动审批拒绝命令，说明实际拒绝原因；不要通过改写命令绕过。项目规则不能放宽宿主权限策略。

## 本地检查

以下命令只检查规则，不执行被检查的危险命令，也不调用模型：

```sh
codex execpolicy check --rules .codex/rules/safety.rules -- git reset --hard HEAD
codex execpolicy check --rules .codex/rules/safety.rules -- npm run build
```

前者应得到 `decision: "prompt"`；后者应得到 `matchedRules: []`。没有规则命中不表示整体沙箱已授权。

本次验证：TOML 通过官方 JSON Schema 校验；硬重置、强制清理、npm/Cargo 发布匹配 prompt，普通构建与清理预览不匹配。检查过程中没有执行被检查的危险命令，也没有调用模型。

当前机器的 `codex-cli 0.148.0` 在完整加载配置时报告 `invalid type: map, expected a boolean in features`。同一个二进制在项目外 `/tmp` 也复现，因此不是本次项目文件引入；全局配置未改动。完整运行时加载尚未通过，不能将静态校验说成已确认生效。桌面端应在受信任项目的新任务中确认加载，CLI 的既有兼容问题另行处理。

配置字段以官方文档和安装版本为准，升级 Codex 后如遇配置告警再复核，不每轮重复检查：
- [配置参考](https://developers.openai.com/codex/config-reference/)
- [AGENTS.md 发现规则](https://developers.openai.com/codex/guides/agents-md/)
- [执行规则](https://developers.openai.com/codex/rules/)
