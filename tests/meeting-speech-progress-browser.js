// 在全新 /tests/meetings.html 中通过 agent-browser eval --stdin 执行，使用模拟下载状态。
(async () => {
  const checks = [];
  const until = async (condition, label) => {
    for (let attempt = 0; attempt < 100; attempt++) {
      if (condition()) return;
      await new Promise((resolve) => setTimeout(resolve, 40));
    }
    throw new Error(label);
  };
  const assert = (condition, label) => {
    if (!condition) throw new Error(label);
    checks.push(label);
  };
  const transcription = () => document.querySelector('progress[aria-label="会议语音模型下载进度"]');
  const speaker = () => document.querySelector('progress[aria-label="说话人模型下载进度"]');
  const card = (progress) => progress.closest(".meeting-speech-model");
  const percent = (progress) => card(progress).querySelector(".meeting-speech-progress-percent")?.textContent;
  const setDownload = (model, downloadedBytes, totalBytes) => {
    Object.assign(model, { state: "downloading", downloadedBytes, totalBytes, canUninstall: false });
  };

  await until(() => document.querySelector(".meeting-speech-link"), "等待会议语音设置入口");
  setDownload(window.__speechModel, 768 * 1024, 1024 ** 2);
  setDownload(window.__speakerModel, 256 * 1024, 1024 ** 2);
  document.querySelector(".meeting-speech-link").click();
  await until(() => transcription() && speaker(), "等待两张模型卡显示下载进度");
  assert(percent(transcription()) === "75%" && transcription().value === 75, "X-ASR 显示 75% 下载进度");
  assert(percent(speaker()) === "25%" && speaker().value === 25, "说话人模型独立显示 25% 下载进度");
  assert(card(transcription()).textContent.includes("已下载 768.0 KiB / 1.0 MiB"), "保留已下载大小和总大小");

  setDownload(window.__speechModel, -1, 1024 ** 2);
  setDownload(window.__speakerModel, 256 * 1024, 0);
  await until(() => percent(transcription()) === "0%" && !speaker().hasAttribute("value"), "等待轮询更新进度");
  assert(transcription().value === 0 && card(transcription()).textContent.includes("已下载 0 B"), "负数进度钳制为 0%");
  assert(percent(speaker()) === undefined && card(speaker()).textContent.includes("已下载 256.0 KiB ·"), "总大小未知时显示不定进度并保留已下载大小");

  setDownload(window.__speechModel, 1024 ** 2, 1024 ** 2);
  setDownload(window.__speakerModel, 1024 ** 2 + 1, 1024 ** 2);
  await until(() => percent(transcription()) === "100%" && percent(speaker()) === "100%", "等待轮询更新校验状态");
  for (const progress of [transcription(), speaker()]) {
    assert(progress.value === 100 && card(progress).textContent.includes("正在校验并安装模型…"),
      `${progress.getAttribute("aria-label")}上限为 100%，并提示校验安装`);
  }
  return { passed: checks.length, checks };
})();
