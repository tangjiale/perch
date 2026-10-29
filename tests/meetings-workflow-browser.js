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
    field(label).click();
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

  await select("转录方式", "分片转录");
  window.__transcribePending = true;
  button("继续录音").click();
  await until(() => !!button("结束录音"));
  window.__emitAudio(10);
  await until(() => typeof window.__releaseTranscribe === "function");
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
    window.__calls.every((call) => call.args.workspaceId === "fixture"),
    "新增领域操作均绑定当前工作空间",
  );
  window.__workflowChecks = checks;
  return { passed: checks.length, checks, viewport: [innerWidth, innerHeight] };
})();
