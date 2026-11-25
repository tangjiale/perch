// 在全新 /tests/meetings.html 中通过 agent-browser eval --stdin 执行，仅使用模拟 IPC 和音频。
(async () => {
  const checks = [];
  const tick = () => new Promise((resolve) => setTimeout(resolve, 40));
  const until = async (condition, label = "等待界面更新") => {
    for (let attempt = 0; attempt < 100; attempt++) {
      if (condition()) return;
      await tick();
    }
    throw new Error(label);
  };
  const assert = (ok, label) => {
    if (!ok) throw new Error(label);
    checks.push(label);
  };
  const button = (label) =>
    [...document.querySelectorAll("button")].find(
      (el) => el.textContent.trim() === label,
    );
  const tab = (label) =>
    [...document.querySelectorAll(".meeting-tabs button")].find((el) =>
      el.textContent.trim().startsWith(label),
    );
  const field = (label) => document.querySelector(`[aria-label="${label}"]`);
  const calls = (name) => window.__calls.filter((call) => call.name === name);
  const errorIncludes = (text) =>
    [...document.querySelectorAll(".meeting-error")].some((el) =>
      el.textContent.includes(text),
    );
  const meeting = () => window.__meetings[0];
  const fill = async (element, value) => {
    const prototype =
      element.tagName === "TEXTAREA"
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value").set.call(
      element,
      value,
    );
    element.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
  };
  const select = async (label, option) => {
    (field(label) || document.getElementById(label)).click();
    await tick();
    const item = [...document.querySelectorAll('[role="option"]')].find((el) =>
      el.textContent.includes(option),
    );
    if (!item) throw new Error(`缺少选项：${label}/${option}`);
    item.click();
    await tick();
  };

  document.querySelector(".meeting-context summary").click();
  await fill(field("会议背景"), "确认发布范围与交付计划");
  await fill(field("参会人"), "林晓，陈舟\n待核对的参会人");
  await fill(field("会议笔记"), "客户意见尚待核实，不作为已确认结论。");
  await fill(field("会议热词"), "Perch\nNebula");
  tab("会议纪要").click();
  await tick();
  await select("纪要整理方式", "详细");
  window.__saveFail = true;
  button("保存修改").click();
  await until(
    () => errorIncludes("模拟保存失败") && !button("保存修改")?.disabled,
  );
  assert(
    field("会议背景").value === "确认发布范围与交付计划",
    "资料保存失败保留全部草稿",
  );
  window.__saveFail = false;
  button("保存修改").click();
  await until(() => !!button("保存")?.disabled);
  assert(
    meeting().context.participants.length === 3 &&
      meeting().context.summaryStyle === "detailed" &&
      meeting().context.glossary.includes("Nebula"),
    "背景参会人笔记热词与纪要详略通过保存入口提交",
  );
  document.querySelector(".meeting-context summary").click();
  tab("行动项").click();
  await tick();
  assert(
    [...document.querySelectorAll(".meeting-action-list button")].every(
      (el) => el.disabled,
    ),
    "修改资料后旧纪要行动项不可直接执行",
  );
  button("生成纪要与脑图").click();
  await until(
    () =>
      !meeting().analysis.stale && !!document.querySelector(".meeting-minutes"),
  );
  assert(
    document
      .querySelector(".meeting-minutes")
      .textContent.includes("确认发布范围与交付计划") &&
      document
        .querySelector(".meeting-minutes")
        .textContent.includes("待核对的参会人"),
    "新纪要显示背景与参会人",
  );

  tab("行动项").click();
  await tick();
  const firstAction = () => document.querySelector(".meeting-action-list li");
  firstAction().querySelector('input[type="checkbox"]').click();
  await until(
    () =>
      firstAction().querySelector("input").checked &&
      !firstAction().querySelector("input").disabled,
  );
  assert(
    firstAction().querySelector("button").disabled,
    "已完成行动项需取消完成后才能转待办",
  );
  firstAction().querySelector('input[type="checkbox"]').click();
  await until(
    () =>
      !firstAction().querySelector("input").checked &&
      !firstAction().querySelector("input").disabled,
  );
  window.__actionFail = true;
  firstAction().querySelector("button").click();
  await until(
    () =>
      errorIncludes("模拟行动项保存失败") &&
      !firstAction().querySelector("button").disabled,
  );
  assert(
    !meeting().analysis.actionItems[0].taskId && !window.__tasks.length,
    "转任务失败保留行动项且不虚构关联",
  );
  window.__actionFail = false;
  const before = calls("meeting_action_task").length;
  firstAction().querySelector("button").click();
  firstAction().querySelector("button").click();
  await until(() => button("打开任务") && !button("打开任务").disabled);
  assert(
    calls("meeting_action_task").length === before + 1 &&
      window.__tasks.length === 1 &&
      window.__taskRefreshCount === 1,
    "转任务防重复提交并刷新任务列表",
  );
  assert(
    window.__tasks[0].source === "local" && !window.__tasks[0].projectId,
    "行动项默认成为本地任务",
  );
  button("打开任务").click();
  assert(
    window.__openedTask === window.__tasks[0].id,
    "打开任务使用实际关联ID",
  );
  window.__taskRefreshFail = true;
  [...document.querySelectorAll(".meeting-action-list button")]
    .find((el) => el.textContent.trim() === "转为任务")
    .click();
  await until(() => errorIncludes("任务已创建") && window.__tasks.length === 2);
  assert(
    !!meeting().analysis.actionItems[1].taskId && !button("转为任务"),
    "列表刷新失败仍保留已创建任务关联，不能重复创建",
  );
  window.__taskRefreshFail = false;

  tab("转录").click();
  await tick();
  assert(
    [...document.querySelectorAll(".meeting-segment time")]
      .map((el) => el.textContent)
      .join(",") === "00:24,00:12",
    "转录仍按时间倒序显示",
  );
  await fill(field("搜索本场转录"), "重点");
  assert(
    document.querySelectorAll(".meeting-segment").length === 1 &&
      meeting().segments.length === 2,
    "本场搜索仅影响显示，不删改原稿",
  );
  await fill(field("搜索本场转录"), "");
  await select("筛选说话人", "陈舟");
  assert(
    document.querySelectorAll(".meeting-segment").length === 1 &&
      document
        .querySelector(".meeting-segment textarea")
        .value.includes("下周三"),
    "按实际服务返回的说话人筛选",
  );
  field("筛选说话人").click();
  await tick();
  assert(
    ![...document.querySelectorAll('[role="option"]')].some((el) =>
      el.textContent.includes("待核对的参会人"),
    ),
    "参会人名单不会伪装成声纹识别结果",
  );
  [...document.querySelectorAll('[role="option"]')]
    .find((el) => el.textContent.includes("全部说话人"))
    .click();
  await tick();
  HTMLMediaElement.prototype.play = async function () {
    window.__lastPlayedTime = this.currentTime;
  };
  field("从 00:24 回听").click();
  await until(
    () => Math.abs(window.__lastPlayedTime - 24) < 0.1,
    "回听未定位到24秒",
  );
  field("从 00:12 回听").click();
  await until(() => Math.abs(window.__lastPlayedTime - 12) < 0.1);
  assert(
    calls("meeting_audio").length === 1,
    "回听按片段定位并复用整场录音缓存",
  );

  button("继续录音").click();
  await until(() => !!button("结束录音"));
  window.__emitAudio(1);
  await until(() => calls("meeting_realtime_push").length > 0);
  await fill(field("重点备注"), "核对范围，不要遗漏兼容验证");
  const markedAt = meeting().durationMs;
  window.__markerFail = true;
  button("标记重点").click();
  await until(() => !!button("重试标记"));
  const attemptedId = calls("meeting_add_marker").at(-1).args.markerId;
  assert(
    window.__micActive && field("重点备注").value.includes("兼容验证"),
    "重点保存失败保留备注且不中断录音",
  );
  window.__markerFail = false;
  button("重试标记").click();
  await until(() => meeting().markers.length === 1 && !!button("标记重点"));
  assert(!errorIncludes("重点标记保存未确认"), "重点重试成功后清除旧失败提示");
  assert(
    calls("meeting_add_marker").at(-1).args.markerId === attemptedId &&
      meeting().markers[0].atMs === markedAt,
    "重点手动重试沿用ID与已保存音频时间",
  );
  window.__emitAudio(1);
  await until(() => calls("meeting_realtime_push").length > 1);
  button("结束录音").click();
  await until(() => !!button("继续录音") && !button("继续录音").disabled);
  assert(
    meeting().markers.length === 1 &&
      document
        .querySelector(".meeting-markers")
        .textContent.includes("兼容验证"),
    "录音继续与停止后重点完整保留",
  );

  document.querySelector(".meeting-speech-link").click();
  await until(() => document.getElementById("meeting-speech-mode"));
  const modelCard = (id) => document.querySelector(`.meeting-speech-model[data-model="${id}"]`);
  const cardOrder = () => [...document.querySelectorAll(".meeting-speech-model[data-model]")]
    .map((card) => card.dataset.model).join(",");
  const selectedModel = () => document.querySelector(".meeting-speech-model:has(.meeting-speech-saved)")?.dataset.model;
  const localModelField = () => document.getElementById("meeting-speech-local-model");
  const localModelOption = (name) => [...document.querySelectorAll('[role="option"]')]
    .find((item) => item.textContent.includes(name));
  const closeModelOptions = async () => {
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await until(() => !localModelOption("X-ASR 480 ms"));
  };
  const speakerToggle = () => document.querySelector(".meeting-speech-speaker-toggle input");
  const recommended = modelCard("r2t2").querySelector("h3");
  assert(modelCard("qwen3-asr-0.6b") && modelCard("x-asr-480") && modelCard("zipformer-zh") &&
    recommended.textContent.includes("有道 Confucius4-R2T2") &&
    recommended.querySelector(".meeting-speech-recommended")?.textContent === "推荐",
    "五款模型分别展示，推荐标签位于有道 Confucius4-R2T2 标题旁");
  assert(localModelField().textContent.includes("X-ASR 160 ms") && selectedModel() === "transcription" &&
    !window.__meetingSpeechPreferences.localModelId && calls("save_meeting_speech_preferences").length === 0,
    "打开旧设置时仍使用 X-ASR 160 ms，推荐标签不写入偏好");
  const r2t2Download = () => field("下载 有道 Confucius4-R2T2");
  await until(() => r2t2Download()?.disabled === true);
  r2t2Download().click();
  assert(modelCard("r2t2").textContent.includes("模拟设备缺少 Vulkan 支持") &&
    calls("install_meeting_speech_model").length === 0, "不支持的设备显示原因并禁用 R2T2 下载");
  assert(cardOrder() === "r2t2,transcription,qwen3-asr-0.6b,x-asr-480,zipformer-zh",
    "推荐模型置顶，已下载模型紧随其后");
  localModelField().click();
  await until(() => localModelOption("Zipformer 中文"));
  const missing = localModelOption("Zipformer 中文");
  missing.click();
  assert(missing.getAttribute("aria-disabled") === "true" &&
    localModelOption("X-ASR 160 ms").getAttribute("aria-disabled") !== "true" &&
    localModelField().textContent.includes("X-ASR 160 ms"),
    "未下载选项置灰且无法选中，已下载模型可以选择");
  await closeModelOptions();
  await select("meeting-microphone-device", "USB 麦克风");
  const savedAudioBeforeCheck = calls("meeting_append_audio").length;
  button("检测麦克风").click();
  await until(() => Number(document.querySelector('[role="meter"]')?.getAttribute("aria-valuenow")) > 0);
  assert(
    window.__micConstraints.at(-1).audio.deviceId.exact === "fixture-usb" &&
      calls("meeting_append_audio").length === savedAudioBeforeCheck,
    "麦克风检测读取所选设备的音量且不创建会议录音",
  );
  button("结束检测").click();
  assert(!window.__micActive, "结束检测释放麦克风");
  await select("meeting-speech-mode", "API 转录");
  await select("meeting-speech-api-model", "支持说话人识别的服务");
  button("保存设置").click();
  await until(() => document.body.textContent.includes("设置已保存"));
  assert(window.__meetingSpeechPreferences.inputDeviceId === "fixture-usb", "输入设备随会议语音设置保存");
  button("检测麦克风").click();
  await until(() => window.__micActive);
  [...document.querySelectorAll("a")].find((link) => link.textContent === "返回会议").click();
  await until(() => field("录音模式")?.textContent.includes("API 分片转录"));
  assert(!window.__micActive, "离开设置自动结束检测");
  assert(
    !!button("重新转录并识别人声") && !document.querySelector(".meeting-retranscribe-bar [role=combobox]"),
    "会议页从设置读取所选 API 和说话人能力，无重复的模型选择",
  );
  window.__transcribePending = true;
  button("继续录音").click();
  await until(() => !!button("结束录音"));
  assert(window.__micConstraints.at(-1).audio.deviceId.exact === "fixture-usb", "正式录音使用保存的麦克风");
  window.__emitAudio(10);
  await until(() => typeof window.__releaseTranscribe === "function");
  assert(calls("meeting_transcribe").at(-1).args.modelId === "speech-diarized", "API 分片使用设置中指定的模型");
  assert(
    button("标记重点").disabled,
    "普通分片识别在途时禁止打点，保护转录版本",
  );
  window.__transcribePending = false;
  window.__releaseTranscribe();
  await until(() => !button("标记重点").disabled);
  window.__markerPending = true;
  await fill(field("重点备注"), "第二个重点");
  button("标记重点").click();
  await until(() => typeof window.__releaseMarker === "function");
  const transcriptions = calls("meeting_transcribe").length;
  const chunks = calls("meeting_append_audio").length;
  window.__emitAudio(10);
  await until(() => calls("meeting_append_audio").length > chunks);
  await tick();
  assert(
    calls("meeting_transcribe").length === transcriptions,
    "正在保存的重点完成前不启动下一分片识别",
  );
  window.__markerPending = false;
  window.__releaseMarker();
  await until(() => calls("meeting_transcribe").length > transcriptions);
  button("结束录音").click();
  await until(() => !!button("继续录音") && !button("继续录音").disabled);
  assert(meeting().markers.length === 2, "串行打点与分片识别都完成且无丢失");
  assert(
    window.__calls
      .filter((call) => !["meeting_speech_status", "meeting_speech_recommendation", "meeting_speaker_model_status"].includes(call.name))
      .every((call) => call.args.workspaceId === "fixture"),
    "新增领域操作均绑定当前工作空间",
  );

  Object.assign(window.__speakerModel, { state: "ready", canUninstall: true });
  document.querySelector(".meeting-speech-link").click();
  const uninstall = (model) => field(`卸载 ${model}`);
  const speakerModel = "Pyannote + CAM++";
  const legacyModel = "X-ASR 160 ms";
  await until(() => uninstall(legacyModel) && uninstall(speakerModel));
  const modelPaths = [...document.querySelectorAll(".meeting-speech-model-path code")].map((element) => element.textContent);
  assert(modelPaths.includes(window.__speechModel.modelPath) && modelPaths.includes(window.__speakerModel.modelPath),
    "API 转录模式仍显示本机模型的独立存储目录和卸载入口");
  uninstall(speakerModel).click();
  await until(() => button("确认卸载"));
  button("取消").click();
  await until(() => !button("确认卸载"));
  assert(calls("uninstall_meeting_speech_model").length === 0, "取消确认时不卸载模型");
  uninstall(speakerModel).click();
  await until(() => button("确认卸载"));
  window.__modelUninstallFail = true;
  button("确认卸载").click();
  await until(() => document.body.textContent.includes("模拟模型正在使用") && button("确认卸载")?.disabled === false);
  assert(window.__speakerModel.state === "ready", "后端拒绝卸载时保留模型并显示原因");
  window.__modelUninstallFail = false;
  await until(() => {
    const confirm = button("确认卸载");
    if (confirm?.disabled !== false) return false;
    confirm.click();
    return true;
  });
  await until(() => !uninstall(speakerModel) && !button("确认卸载"));
  assert(calls("uninstall_meeting_speech_model").at(-1).args.model === "speaker" && window.__speechModel.state === "ready",
    "卸载说话人模型时保留 X-ASR");
  uninstall(legacyModel).click();
  await until(() => button("确认卸载"));
  button("确认卸载").click();
  await until(() => !uninstall(legacyModel) && field(`下载 ${legacyModel}`));
  assert(calls("uninstall_meeting_speech_model").at(-1).args.model === "transcription" && window.__speechModel.runtime === "cold",
    "卸载 X-ASR 后可重新下载");

  await select("meeting-speech-mode", "本机实时转录");
  speakerToggle().click();
  assert(speakerToggle().checked && window.__meetingSpeechPreferences.mode === "api",
    "草稿中的说话人勾选不修改已保存模式");
  field("下载 X-ASR 480 ms").click();
  await until(() => modelCard("x-asr-480").querySelector("progress")?.value === 60);
  assert(calls("install_meeting_speech_model").at(-1).args.model === "x-asr-480" &&
    window.__speechModel.state === "missing", "仅下载指定的 X-ASR 480 ms，保留独立进度");
  localModelField().click();
  await until(() => localModelOption("X-ASR 480 ms"));
  assert(localModelOption("X-ASR 480 ms").getAttribute("aria-disabled") === "true",
    "下载中的模型仍不可选择");
  const warmupCalls = calls("prepare_meeting_speech_model").length;
  Object.assign(window.__speechModels["x-asr-480"], { state: "ready", canUninstall: true });
  await until(() => localModelOption("X-ASR 480 ms")?.getAttribute("aria-disabled") !== "true" &&
    modelCard("x-asr-480").textContent.includes("已安装"));
  assert(cardOrder() === "r2t2,x-asr-480,qwen3-asr-0.6b,transcription,zipformer-zh",
    "下载完成后选项立即可用，卡片移到未下载模型前面");
  assert(!selectedModel() && calls("prepare_meeting_speech_model").length === warmupCalls &&
    window.__meetingSpeechPreferences.mode === "api", "下载安装完成不会自动选用或预加载模型");
  localModelOption("X-ASR 480 ms").click();
  await until(() => localModelField().textContent.includes("X-ASR 480 ms"));
  button("保存设置").click();
  await until(() => selectedModel() === "x-asr-480" && window.__speechModels["x-asr-480"].runtime === "ready");
  const preferences = calls("save_meeting_speech_preferences").at(-1).args.value;
  assert(preferences.localModelId === "x-asr-480" && preferences.localSpeakerDiarization === true &&
    preferences.inputDeviceId === "fixture-usb", "保存分别提交本机模型、说话人开关和麦克风");

  const beforeReturn = calls("meeting_speech_status").length;
  [...document.querySelectorAll("a")].find((link) => link.textContent === "返回会议").click();
  const recordingStatus = () => document.querySelector(".meeting-state")?.textContent ?? "";
  await until(() => recordingStatus().includes("X-ASR 480 ms · 已就绪"));
  const statusCalls = calls("meeting_speech_status").slice(beforeReturn);
  assert(statusCalls.length > 0 && statusCalls.every((call) => call.args.model === "x-asr-480"),
    "会议页查询已保存的模型，并显示同一模型名称");
  const startCheckCount = calls("meeting_speech_status").length;
  button("继续录音").click();
  await until(() => !!button("结束录音"));
  assert(calls("meeting_speech_status").slice(startCheckCount).some((call) => call.args.model === "x-asr-480") &&
    recordingStatus().includes("X-ASR 480 ms"), "起录再次检查所选模型，录音状态显示其名称");
  button("结束录音").click();
  await until(() => !!button("继续录音") && !button("继续录音").disabled);
  document.querySelector(".meeting-speech-link").click();
  await until(() => uninstall("X-ASR 480 ms") && speakerToggle()?.checked);
  assert(localModelField().textContent.includes("X-ASR 480 ms"), "重开设置恢复已保存的模型和说话人开关");
  uninstall("X-ASR 480 ms").click();
  await until(() => button("确认卸载"));
  button("确认卸载").click();
  await until(() => !uninstall("X-ASR 480 ms") && field("下载 X-ASR 480 ms"));
  assert(calls("uninstall_meeting_speech_model").at(-1).args.model === "x-asr-480" && selectedModel() === "x-asr-480",
    "按模型 ID 卸载，新录音偏好保持所选模型");
  localModelField().click();
  await until(() => localModelOption("X-ASR 480 ms"));
  assert(localModelOption("X-ASR 480 ms").getAttribute("aria-disabled") === "true" &&
    cardOrder() === "r2t2,qwen3-asr-0.6b,transcription,x-asr-480,zipformer-zh",
    "卸载后恢复禁选并回到未下载组");
  await closeModelOptions();
  window.__workflowChecks = checks;
  return { passed: checks.length, checks, viewport: [innerWidth, innerHeight] };
})();
