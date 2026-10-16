# 栖点软件操作讲解视频

105.13 秒，1920×1080，30fps，H.264 / AAC。全片只显示栖点软件界面，连续展示菜单点击、表单填写、保存结果与视图切换；局部放大跟随操作区域。没有官网画面、品牌标题页或网站地址结尾。

## 预览与导出

```sh
cd video
npm ci
npm run dev
npm run lint
npm run sync:site
npm run render
npm run poster
```

需要本机 `ffmpeg`。Studio 打印本地地址，选择 `PerchIntro` 预览。输出到 `docs/assets/perch-intro.mp4`、`perch-intro-poster.jpg` 和 `perch-intro.zh-CN.vtt`，使用已有 GitHub Pages 工作流部署；独立 npm 工程，不改变桌面应用依赖或版本。

## 时间线与录屏

`capture-manifest.json` 保存六段原始录屏、逐步操作时刻和镜头焦点；`voice-manifest.json` 保存 21 句配音与实际时长。字幕和官网章节由最终时间线生成。画面放大使用 Remotion 帧动画，鼠标与界面统一缩放，防止指针偏离点击目标。表单与聊天段把字幕置于上方，避免挡住底部操作按钮。

- 工作总览 → 创建与推进任务 → 日历排期 → AI 工作伙伴 → 日常工具 → 数据与备份。
- `public/recordings/` 是 agent-browser 带鼠标捕获的真实当前 React 软件录屏，不是截图切换。
- `tests/video-demo.html` 与 `tests/VideoDemoFixture.ts` 提供可操作的匿名内存演示。只允许本地任务、会话元数据和展示用读取；拒绝真实 AI 发送、禅道回写和备份执行，刷新后内存数据重置。
- 新建任务、提醒表单与状态变更的结果来自内存演示；不能作为原生持久化或通知投递的验证。聊天只展示选择知识库、输入和发送入口，没有虚构真实模型响应。
- 原始录屏 1600×900。用 `npm run dev` 启动根项目后，以 `agent-browser --session perch-operation` 打开 `http://127.0.0.1:1420/tests/video-demo.html#/`，设置相同窗口尺寸。按 `overview tasks calendar chat tools backup` 顺序运行 `python3 video/scripts/capture-demo.py <场景>`。须在一次页面生命周期内录制，不在场景之间刷新。
- `capture-demo.py` 通过真实鼠标点击和输入事件操作界面；日期控件使用浏览器标准 setter 与 input/change 事件适配原生日期输入。选择器失效会中止，录屏失败不能进入时间线。

## 配音来源

旁白为用户明确同意的 **温柔桃子（升级版）参考音频近似合成**，不是豆包官方原版。使用本机 `Qwen3-TTS-12Hz-0.6B-Base-4bit`，参考录音仅本地读取，没有上传第三方。成片使用 21 句独立音频，不使用上一版 Tingting 系统语音，不添加背景音乐。

文稿位于 `narration-script.json`。所有音频保持生成语速与音高，完整保留语音并追加短尾静音；已用本地 Whisper 逐句核验，字幕以原稿字形为准。生成脚本与依赖说明位于 `scripts/clone-voice*`，已提交音频可直接渲染，不需要安装语音模型。

重新合成需要 Apple Silicon 上独立 Python 环境，以及已下载的本地模型与用户授权参考目录：

```sh
python3 scripts/clone-voice-batch.py \
  --model /absolute/path/to/local-qwen3-tts-model \
  --reference-dir /absolute/path/to/authorized-reference
# 可选 --asr-model /absolute/path/to/local-whisper-model
```

脚本强制离线，不自动下载模型。不要把参考原录音或本地模型提交到仓库。更新音频后须重核实际时长、录屏停顿、字幕和章节。

原第一版截图视频与本次 QA 文件仅保留在被 Git 忽略的 `out/`。Remotion 使用遵循其官方许可：https://www.remotion.dev/license 。
