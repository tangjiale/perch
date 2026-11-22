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
  const rows = () => [...document.querySelectorAll(".meeting-segment")];
  const rowText = (row) => row.querySelector("textarea").value;
  const findRow = (text) => rows().find((row) => rowText(row).includes(text));
  const field = (label) => document.querySelector(`[aria-label="${label}"]`);
  const fillSearch = async (text) => {
    const input = field("搜索本场转录");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
  };
  const filterSpeaker = async (label) => {
    field("筛选说话人").click();
    await until(() => document.querySelector('[role="option"]'));
    const option = [...document.querySelectorAll('[role="option"]')].find((el) => el.textContent.includes(label));
    assert(option, `说话人筛选项包括 ${label}`);
    option.click();
    await tick();
  };
  const emitEvent = (session, event) => {
    session.onEvent.onmessage({
      type: "delta", sessionId: session.sessionId, workspaceId: session.workspaceId, meetingId: session.id, ...event,
    });
  };
  const makeLine = (session, sequence, text, speaker, start, end) => ({
    id: `realtime:${session.sessionId}:${sequence}`,
    text, speaker,
    startMs: session.startMs + start,
    endMs: session.startMs + end,
  });
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
  await until(() => findRow("实时识别第1句"));
  const initialRow = findRow("实时识别第1句");
  assert(window.__micActive && rows().length === 3 && initialRow === rows()[0], "录音中把最新实时行和已保存片段显示在同一张倒序列表");
  assert(initialRow.querySelector("textarea").readOnly && !initialRow.querySelector("textarea").disabled
    && initialRow.querySelector("button").disabled, "录音中实时片段可选择文字但不能编辑或回听");
  assert(initialRow.querySelector("time").textContent === "21:00", "续录行使用包含偏移的会议时间");
  assert(window.__meetings[0].segments.length === 2, "实时片段未写入已保存的转录原稿");
  const audioIndex = window.__calls.findIndex((call) => call.name === "meeting_append_audio");
  const pushIndex = window.__calls.findIndex((call) => call.name === "meeting_realtime_push");
  assert(audioIndex >= 0 && pushIndex > audioIndex, "音频先保存再送入实时服务");

  const lines = [
    makeLine(firstSession, 0, "开始会议。", "local-speaker:1", 0, 50),
    makeLine(firstSession, 1, "我补充一下。", "local-speaker:2", 50, 100),
    makeLine(firstSession, 2, "继续。", "local-speaker:1", 100, 150),
  ];
  window.__emitRealtime(lines.slice(0, 2));
  await until(() => findRow("我补充一下。"));
  assert(findRow("开始会议。") === initialRow, "同 ID 的文字和说话人更新保留原来的行");
  window.__emitRealtime(lines);
  await until(() => findRow("继续。"));
  const labels = rows().slice(0, 3).map((row) => row.querySelector("input").value);
  assert(window.__micActive && labels.join(",") === "说话人 1,说话人 2,说话人 1", "A/B/A 穿插发言复用相同的说话人编号");
  assert(document.querySelector('.meeting-tabs button[aria-current="page"] span').textContent === "5"
    && document.querySelector(".meeting-transcript-tools").textContent.includes("5 / 5 段"), "转录角标和列表计数包含历史与实时片段");
  await fillSearch("补充");
  assert(rows().length === 1 && findRow("我补充一下。"), "列表搜索包含实时片段");
  await fillSearch("");
  await filterSpeaker("说话人 1 · 2 段");
  assert(rows().length === 2 && rows().every((row) => row.querySelector("input").value === "说话人 1"), "按说话人筛选实时片段");
  await filterSpeaker("全部说话人 · 5 段");

  emitEvent(firstSession, { type: "speakerError", message: "模拟说话人模型加载失败" });
  await until(() => document.querySelector(".meeting-record-status").textContent.includes("实时文字转录继续"));
  assert(window.__micActive && window.__realtimeSessions.size > 0, "说话人模型异常不影响实时转录或录音");
  emitEvent(firstSession, { text: "无法取得结构时的备用文字。" });
  await until(() => findRow("无法取得结构时的备用文字。"));
  assert(rows().length === 3, "仅收到 text 时以普通列表行展示容错内容");
  const longText = "连续发言不用独立预览。\n".repeat(45);
  emitEvent(firstSession, { text: longText });
  await until(() => findRow("连续发言不用独立预览。"));
  const longInput = rows()[0].querySelector("textarea");
  const detail = document.querySelector(".meeting-detail");
  assert(longInput.scrollHeight - longInput.clientHeight <= 1 && detail.scrollHeight > detail.clientHeight,
    "长段落自动展开并随会议详情正常滚动");
  emitEvent(firstSession, { lines: [], text: "" });
  await until(() => rows().length === 2);
  assert(!document.querySelector(".meeting-realtime"), "空的实时快照只移除实时行，无独立预览区");
  window.__emitRealtime(lines);
  await until(() => findRow("继续。"));
  const rowBeforeStop = findRow("继续。");
  emitEvent(firstSession, { workspaceId: "another-workspace", text: "错误工作空间内容" });
  emitEvent(firstSession, { sessionId: "expired-session", text: "过期会话内容" });
  emitEvent(firstSession, { meetingId: "another-meeting", text: "其他会议内容" });
  await tick();
  assert(rows().length === 5 && findRow("继续。") === rowBeforeStop, "忽略其他工作空间、会议和过期会话的事件");
  window.__emitAudio(0.07);
  button("结束录音").click();
  await until(() => !window.__micActive && ready() && window.__meetings[0].segments.length === 5);
  assert(window.__meetings[0].durationMs === baseDuration + 230, "停止时完整保存不足 160 毫秒的尾部音频");
  const finalRow = findRow("继续。实时识别第2句。");
  assert(rows().length === 5 && finalRow === rowBeforeStop && !finalRow.querySelector("textarea").readOnly
    && !finalRow.querySelector("button").disabled, "收尾时保留同一行并切换为可编辑可回听，无重复行");
  emitEvent(firstSession, { text: "已关闭会话迟到结果" });
  await tick();
  assert(!findRow("已关闭会话"), "收尾后迟到结果不会重新污染页面");

  button("继续录音").click();
  await until(() => button("结束录音"));
  const secondSession = window.__realtimeSessions.get(window.__lastRealtimeSession);
  window.__emitAudio(0.16);
  await until(() => window.__calls.filter((call) => call.name === "meeting_realtime_push" && call.args.sessionId === secondSession.sessionId).length === 1);
  window.__emitRealtime([
    makeLine(secondSession, 0, "回读后确认已保存。", "local-speaker:1", 0, 80),
    makeLine(secondSession, 1, "尚未确认的片段。", "local-speaker:2", 80, 160),
  ]);
  await until(() => findRow("尚未确认的片段"));
  window.__realtimePushFail = "模拟识别连接中断";
  window.__realtimeUncommittedCount = 1;
  window.__emitAudio(0.16);
  await until(() => document.body.textContent.includes("模拟识别连接中断"));
  const pushesAtFailure = calls("meeting_realtime_push").length;
  const chunksAtFailure = window.__audioChunks.size;
  window.__emitAudio(0.16);
  await until(() => window.__audioChunks.size > chunksAtFailure);
  assert(window.__micActive && calls("meeting_realtime_push").length === pushesAtFailure, "识别中断继续保存录音且不自动重发");
  assert(findRow("尚未确认的片段"), "识别中断保留已收到的文字");
  button("结束录音").click();
  await until(() => ready() && !window.__micActive);
  const confirmedRows = rows().filter((row) => rowText(row).includes("回读后确认已保存"));
  const unsavedInput = findRow("尚未确认的片段").querySelector("textarea");
  assert(window.__meetings[0].segments.length === 6 && calls("meeting_list").length >= 2
    && confirmedRows.length === 1 && !confirmedRows[0].querySelector("textarea").readOnly, "异常回读后与已持久化的片段按 ID 去重");
  unsavedInput.select();
  assert(unsavedInput.readOnly && !unsavedInput.disabled && unsavedInput.selectionEnd === unsavedInput.value.length
    && findRow("尚未确认的片段").querySelector("button").disabled
    && document.querySelector(".meeting-content").textContent.includes("1 段转录尚未确认保存"), "未确认的片段保持只读并可选择完整文字复制");
  assert(document.body.textContent.includes("模拟识别连接中断"), "异常收尾保留明确失败提示");
  window.__realtimePushFail = "";
  assert(calls("meeting_transcribe").length === 0, "实时录音不偷偷退回HTTP分片接口");

  button("新建").click();
  await until(() => document.querySelector('[aria-label="会议名称"]').value === "未命名会议");
  assert(rows().length === 0, "新会议不会显示其他会议未确认的实时行");
  const oldMeeting = [...document.querySelectorAll(".meeting-list-item")].find((el) => el.textContent.includes("产品迭代评审"));
  oldMeeting.click();
  await until(() => findRow("尚未确认的片段"));
  assert(findRow("尚未确认的片段").querySelector("textarea").readOnly, "返回原会议仍能复制未确认保存的实时行");
  [...document.querySelectorAll(".meeting-list-item")].find((el) => el.textContent.includes("未命名会议")).click();
  await tick();
  window.__micFailure = "模拟麦克风权限被拒绝";
  button("开始录音").click();
  await until(() => document.body.textContent.includes("模拟麦克风权限被拒绝") && !button("开始录音").disabled);
  assert(!document.querySelector(".meeting-error").textContent.match(/检查供应商地址|API Key/), "麦克风失败不误报为供应商认证问题");
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
