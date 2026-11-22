// 在全新 /tests/meetings.html 页面通过 agent-browser eval --stdin 执行。
// 只使用内存 IPC、模拟媒体设备。
(async () => {
  const checks = [];
  const tick = () => new Promise((resolve) => setTimeout(resolve, 30));
  const until = async (predicate) => {
    for (let i = 0; i < 100; i++) {
      if (predicate()) return;
      await tick();
    }
    throw new Error("等待界面状态超时");
  };
  const assert = (condition, label) => {
    if (!condition) throw new Error(label);
    checks.push(label);
  };
  const button = (label) => [...document.querySelectorAll("button")].find((el) => el.textContent.trim() === label);
  const ready = () => !!button("继续录音") && !button("继续录音").disabled;
  const calls = (name) => window.__calls.filter((call) => call.name === name);
  const preview = () => document.querySelector(".meeting-realtime")?.textContent || "";
  const baseDuration = window.__meetings[0].durationMs;
  await until(() => document.querySelector('[aria-label="录音模式"]')?.textContent.includes("本机实时转录"));
  assert(ready(), "本机模型就绪后默认选择本机实时转录");
  if (new URLSearchParams(location.search).get("runtime") === "cold") {
    await until(() => window.__speechModel.runtime === "ready");
    assert(calls("prepare_meeting_speech_model").length === 1 && window.__micOpenCount === 0, "会议页后台预加载且不打开麦克风");
  }
  assert(!document.querySelector(".meeting-retranscribe-bar"), "本机模式不显示 API 模型或会后转录操作");

  window.__realtimeStartFail = "本机语音模型加载失败";
  button("继续录音").click();
  await until(() => document.body.textContent.includes("本机语音模型加载失败") && ready());
  assert(window.__micOpenCount === 0, "本机模型加载失败时不打开麦克风");
  assert(calls("meeting_append_audio").length === 0, "加载失败时未产生音频");

  window.__realtimeStartFail = "";
  button("继续录音").click();
  await until(() => button("结束录音"));
  assert(calls("meeting_realtime_start").at(-1).args.modelId === "local", "实时转录不使用供应商模型");
  const firstSession = window.__realtimeSessions.get(window.__lastRealtimeSession);
  window.__emitAudio(0.12);
  await tick();
  assert(calls("meeting_realtime_push").length === 0, "不足 160 毫秒的音频保留至下一块或停止");
  window.__emitAudio(0.04);
  await until(() => preview().includes("实时识别第1句"));
  assert(window.__micActive && !!button("结束录音"), "录音未停止就能看到识别文字");
  assert(window.__meetings[0].segments.length === 2, "实时预览独立于原有转录校对文本");
  const audioIndex = window.__calls.findIndex((call) => call.name === "meeting_append_audio");
  const pushIndex = window.__calls.findIndex((call) => call.name === "meeting_realtime_push");
  assert(audioIndex >= 0 && pushIndex > audioIndex, "音频先保存再送入实时服务");
  const emitPreview = async (text, lines) => {
    firstSession.onEvent.onmessage({ type: "delta", sessionId: firstSession.sessionId, workspaceId: firstSession.workspaceId, meetingId: firstSession.id, text, lines });
    await tick();
  };
  await emitPreview("开始会议。 我补充一下。 继续。", [
    {text: "开始会议。", speaker: "local-speaker:1", startMs: 0, endMs: 1200},
    {text: "我补充一下。", speaker: "local-speaker:2", startMs: 1200, endMs: 3000},
    {text: "继续。", speaker: "local-speaker:1", startMs: 3000, endMs: 4000},
  ]);
  const labels = [...document.querySelectorAll(".meeting-realtime-line strong")].map((label) => label.textContent);
  assert(window.__micActive && labels.join(",") === "说话人 1,说话人 2,说话人 1", "录音中区分穿插发言并复用说话人编号");
  firstSession.onEvent.onmessage({ type: "speakerError", sessionId: firstSession.sessionId, workspaceId: firstSession.workspaceId, meetingId: firstSession.id, message: "模拟说话人模型加载失败" });
  await until(() => preview().includes("实时文字转录继续"));
  assert(window.__micActive && window.__realtimeSessions.size > 0, "说话人模型异常不影响实时转录或录音");
  const longPreview = Array.from({ length: 100 }, (_, i) => `第${i + 1}句会议识别内容。`).join("\n");
  await emitPreview(longPreview);
  const previewText = document.querySelector(".meeting-realtime-text");
  assert(previewText.scrollHeight > previewText.clientHeight && previewText.clientHeight <= 280, "长转录限制高度并在文字区内滚动");
  assert(previewText.scrollHeight - previewText.scrollTop - previewText.clientHeight < 2, "默认自动跟随最新文字");
  previewText.scrollTop = 0;
  previewText.dispatchEvent(new Event("scroll", { bubbles: true }));
  await emitPreview(longPreview + "\n新增一句内容。");
  assert(previewText.scrollTop === 0, "阅读旧文时新增文字不强制拉到底部");
  previewText.scrollTop = previewText.scrollHeight;
  previewText.dispatchEvent(new Event("scroll", { bubbles: true }));
  await emitPreview(longPreview + "\n新增一句内容。\n最新识别内容。");
  assert(previewText.scrollHeight - previewText.scrollTop - previewText.clientHeight < 2, "回到底部后恢复自动跟随");
  await emitPreview("替换后的累计快照。");
  assert(previewText.textContent === "替换后的累计快照。", "累计文字快照替换展示且不重复叠加");
  await emitPreview("");
  assert(!document.querySelector(".meeting-realtime-text"), "空快照保持状态提示且不占用文字区域");
  await emitPreview("实时识别第1句。");
  firstSession.onEvent.onmessage({ type: "delta", sessionId: firstSession.sessionId, workspaceId: "another-workspace", meetingId: firstSession.id, text: "错误工作空间内容" });
  firstSession.onEvent.onmessage({ type: "delta", sessionId: "expired-session", workspaceId: firstSession.workspaceId, meetingId: firstSession.id, text: "过期会话内容" });
  await tick();
  assert(!preview().includes("错误工作空间") && !preview().includes("过期会话"), "过滤其他工作空间和过期会话的事件");
  window.__emitAudio(0.07);
  button("结束录音").click();
  await until(() => !window.__micActive && ready() && window.__meetings[0].segments.length === 3);
  assert(window.__meetings[0].durationMs === baseDuration + 230, "停止时完整保存不足 160 毫秒的尾部音频");
  assert(window.__meetings[0].segments[2].text === "实时识别第1句。实时识别第2句。", "停止后持久化最终全文且保留原段落");
  assert(!document.querySelector(".meeting-realtime"), "成功收尾后移除临时预览");
  firstSession.onEvent.onmessage({ type: "delta", sessionId: firstSession.sessionId, workspaceId: firstSession.workspaceId, meetingId: firstSession.id, text: "已关闭会话迟到结果" });
  await tick();
  assert(!preview().includes("已关闭会话"), "收尾后迟到结果不会重新污染页面");

  button("继续录音").click();
  await until(() => button("结束录音"));
  window.__emitAudio(0.16);
  await until(() => preview().includes("实时识别第1句"));
  window.__realtimePushFail = "模拟识别连接中断";
  window.__emitAudio(0.16);
  await until(() => document.body.textContent.includes("模拟识别连接中断"));
  const pushesAtFailure = calls("meeting_realtime_push").length;
  const chunksAtFailure = window.__audioChunks.size;
  window.__emitAudio(0.16);
  await until(() => window.__audioChunks.size > chunksAtFailure);
  assert(window.__micActive && calls("meeting_realtime_push").length === pushesAtFailure, "识别中断继续保存录音且不自动重发");
  assert(preview().includes("实时识别第1句"), "识别中断保留已收到的文字");
  button("结束录音").click();
  await until(() => ready() && !window.__micActive);
  assert(window.__meetings[0].segments.length === 4 && calls("meeting_list").length >= 2, "异常收尾后回读已持久化的部分文字");
  assert(document.body.textContent.includes("模拟识别连接中断"), "异常收尾保留明确失败提示");
  window.__realtimePushFail = "";
  assert(calls("meeting_transcribe").length === 0, "实时录音不偷偷退回HTTP分片接口");

  button("新建").click();
  await until(() => document.querySelector('[aria-label="会议名称"]').value === "未命名会议");
  assert(!document.querySelector(".meeting-realtime"), "新会议不会显示其他会议的异常预览");
  const oldMeeting = [...document.querySelectorAll(".meeting-list-item")].find((el) => el.textContent.includes("产品迭代评审"));
  oldMeeting.click();
  await until(() => preview().includes("实时识别第1句"));
  assert(preview().includes("实时识别第1句"), "返回原会议仍保留可复制的异常预览");
  [...document.querySelectorAll(".meeting-list-item")].find((el) => el.textContent.includes("未命名会议")).click();
  await tick();
  window.__micFailure = "模拟麦克风权限被拒绝";
  button("开始录音").click();
  await until(() => document.body.textContent.includes("模拟麦克风权限被拒绝") && !button("开始录音").disabled);
  assert(!preview().includes("检查供应商地址") && !preview().includes("API Key"), "麦克风失败不误报为供应商认证问题");
  assert(!window.__micActive && window.__realtimeSessions.size === 0, "麦克风失败释放已连接的实时会话");
  window.__micFailure = "";
  button("开始录音").click();
  await until(() => button("结束录音"));
  const stopsBeforeDisconnect = calls("meeting_realtime_stop").length;
  window.__emitAudio(0.5);
  window.__disconnectMicrophone();
  await until(() => ready() && !window.__micActive);
  assert(calls("meeting_realtime_stop").length === stopsBeforeDisconnect + 1, "设备断开触发一次完整收尾");
  assert(window.__meetings[0].durationMs === 500 && window.__meetings[0].segments.length === 1, "设备断开仍保存尾部音频与识别结果");
  assert(document.documentElement.scrollWidth <= innerWidth, "实时界面无横向溢出");
  return { passed: checks.length, checks, viewport: [innerWidth, innerHeight] };
})()
