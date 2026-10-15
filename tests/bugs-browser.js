(async () => {
  if (location.pathname !== "/tests/bugs.html")
    throw Error("仅允许在隔离 BUG fixture 中执行");
  const { default: interactionScript } = await import('/src-tauri/src/desktop-interaction.js?raw');
  new Function(interactionScript)();
  const chineseImageMenu = async image => {
    image.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 }));
    await wait();
    const menu = document.querySelector('[role=menu][aria-label=图片操作]');
    const valid = menu?.textContent.includes('复制图片') && menu.textContent.includes('下载图片');
    menu?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await wait();
    return valid;
  };
  const wait = () => new Promise((resolve) => setTimeout(resolve, 150));
  const button = (text, scope = document) =>
    [...scope.querySelectorAll("button")].find(
      (item) => item.textContent.trim() === text,
    );
  const check = (condition, message) => {
    if (!condition) throw Error(message);
  };
  await wait();
  check(document.querySelectorAll("tbody tr").length === 1, "默认仅显示待解决 BUG");
  check(document.querySelectorAll(".bugs-status-filters button")[1].getAttribute("aria-pressed") === "true", "默认选中待解决");
  document.querySelectorAll(".bugs-status-filters button")[0].click();
  await wait();
  check(document.querySelector("tbody tr").textContent.includes("#102"), "默认创建时间倒序");
  for (const label of ["状态", "优先级", "严重程度", "创建", "截止日期"]) {
    const header = [...document.querySelectorAll(".bugs-sort-button")].find((item) => item.textContent === label);
    header.click();
    await wait();
    check(header.closest("th").getAttribute("aria-sort") === "ascending", `${label}升序`);
    header.click();
    await wait();
    check(header.closest("th").getAttribute("aria-sort") === "descending", `${label}倒序`);
  }
  button("状态").click();
  await wait();
  check(
    document.querySelectorAll("tbody tr").length === 3,
    "应展示三个状态的我的 BUG",
  );
  window.__bugImageFail = true;
  document.querySelector(".bugs-title").click();
  await wait();
  let dialog = document.querySelector("dialog");
  check(
    dialog.querySelector(".bugs-steps").textContent.includes("附件加载失败"),
    "纯截图复现步骤失败时显示提示，不能留空",
  );
  window.__bugImageFail = false;
  button("重试加载", dialog).click();
  await wait();
  check(dialog.querySelector(".bugs-steps img")?.naturalWidth === 1400, "纯截图复现步骤加载成功");
  check(await chineseImageMenu(dialog.querySelector(".bugs-steps img")), "详情截图应显示中文图片菜单");
  check(!dialog.querySelector(".bugs-steps a, .bugs-steps script, .bugs-steps [onerror]"), "危险内容不进入详情 DOM");
  check([...dialog.querySelectorAll(".bugs-steps img")].every(image => image.src.startsWith("data:image/")), "图片仅使用后端返回的 data URL");
  dialog.querySelector('[aria-label="放大查看复现步骤图片"]').click();
  await wait();
  check(document.querySelectorAll("dialog[open]").length === 2, "截图可放大预览");
  const preview = document.querySelectorAll("dialog[open]")[1];
  check(await chineseImageMenu(preview.querySelector("img")), "大图预览应显示中文图片菜单");
  button("原始尺寸", preview).click();
  await wait();
  check(Number(preview.querySelector(".image-viewer-stage").dataset.scale) === 1, "支持原始尺寸查看");
  preview.querySelector('[title="关闭"]').click();
  await wait();
  check(document.querySelectorAll("dialog[open]").length === 1, "关闭预览仍保留BUG详情");
  check(!window.__unsafeBug, "复现内容不应执行脚本");
  button("编辑 BUG", dialog).click();
  await wait();
  check(!dialog.querySelector("input").disabled, "标题应可编辑");
  const editor = dialog.querySelector('[aria-label="BUG 复现步骤富文本"]');
  check(editor?.getAttribute("contenteditable") === "true" && !dialog.querySelector("textarea"), "复现步骤使用富文本，不能暴露HTML源码");
  check(editor.querySelector("img")?.src.startsWith("data:image/"), "富文本中截图正常显示");
  check(await chineseImageMenu(editor.querySelector("img")), "编辑态截图应显示中文图片菜单");
  const originalSteps = '<p><img src="{708.png}" alt="复现截图" onerror="window.__unsafeBug=true" /></p>';
  editor.querySelector("img").click();
  await wait();
  const editPreview = document.querySelector("dialog.image-viewer[open]");
  check(editPreview, "编辑 BUG 时也能放大截图");
  button("原始尺寸", editPreview).click();
  editPreview.querySelector('[title="关闭"]').click();
  await wait();
  window.__bugFail = true;
  button("保存到禅道", dialog).click();
  await wait();
  check(
    dialog.querySelector('[role="alert"]').textContent.includes("权限不足"),
    "写入失败应保留错误和详情",
  );
  check(!editor.textContent.includes("<p>") && editor.getAttribute("contenteditable") === "true", "失败后富文本仍可编辑");
  check(
    window.__bugCalls.at(-1).name === "zentao_bug_save",
    "保存调用 BUG 领域 IPC",
  );
  check(window.__bugCalls.at(-1).args.value.steps === originalSteps && originalSteps.includes("{708.png}") && !originalSteps.includes("data:image"), "图片加载及预览不改变保存的原始 steps");
  window.__bugFail = false;
  button("取消编辑", dialog).click();
  await wait();
  const mutations = () => window.__bugCalls.filter(call => call.name === "zentao_bug_transition").length;
  const beforeResolve = mutations();
  button("解决", dialog).click();
  await wait();
  check(mutations() === beforeResolve && document.querySelector("dialog[open]"), "点击解决只打开表单，不自动提交空备注");
  const comment = dialog.querySelector('[aria-label="操作备注"]');
  check(comment && !comment.disabled, "解决备注可以填写");
  const commentText = "已调整配置并验证检索恢复\n补充复测说明";
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set.call(comment, commentText);
  comment.dispatchEvent(new Event("input", { bubbles: true }));
  await wait();
  window.__bugFail = true;
  button("确认解决 BUG", dialog).click();
  await wait();
  check(comment.value === commentText && !comment.disabled, "提交失败保留备注并允许继续输入");
  window.__bugFail = false;
  button("确认解决 BUG", dialog).click();
  await wait();
  const resolve = window.__bugCalls.at(-1);
  check(
    resolve.name === "zentao_bug_transition" &&
      resolve.args.action === "resolve" &&
      resolve.args.payload.resolvedBuild === "trunk" &&
      resolve.args.payload.comment === commentText &&
      resolve.args.revision === 1,
    "解决动作携带版本及修订号",
  );
  check(!document.querySelector("dialog"), "成功后关闭详情");
  document.querySelectorAll(".bugs-title")[1].click();
  await wait();
  dialog = document.querySelector("dialog");
  check(dialog.querySelector(".bugs-steps").textContent.includes("测试复现步骤") && dialog.querySelector(".bugs-steps img"), "文字和图片同时展示");
  check(dialog.querySelector(".bugs-steps").textContent.includes("仅支持当前禅道站点"), "外站图片保留明确错误占位");
  check(!performance.getEntriesByType("resource").some(entry => entry.name.includes("tracking.invalid")), "不向外站图片发起请求");
  button("编辑 BUG", dialog).click();
  await wait();
  const richEditor = dialog.querySelector('[aria-label="BUG 复现步骤富文本"]');
  check(richEditor.textContent.includes("测试复现步骤") && !richEditor.textContent.includes("<p>"), "编辑文字按富文本排版");
  richEditor.focus();
  const selection = window.getSelection();
  selection.selectAllChildren(richEditor.querySelector("p"));
  selection.collapseToStart();
  dialog.querySelector('[aria-label="加粗"]').click();
  document.execCommand("insertText", false, "新增复现说明：");
  await wait();
  window.__bugFail = true;
  button("保存到禅道", dialog).click();
  await wait();
  check(richEditor.textContent.includes("新增复现说明：") && richEditor.getAttribute("contenteditable") === "true", "编辑失败保留新文字和输入状态");
  const changedSteps = window.__bugCalls.findLast(call => call.name === "zentao_bug_save").args.value.steps;
  check(changedSteps.includes("<strong>新增复现说明：</strong>") && changedSteps.includes("fileID=709") && !changedSteps.includes("data:image"), "富文本保存格式与原始图片引用，不保存预览数据");
  window.__bugFail = false;
  button("保存到禅道", dialog).click();
  await wait();
  check(!document.querySelector("dialog"), "富文本保存成功关闭详情");
  document.querySelectorAll(".bugs-title")[1].click();
  await wait();
  dialog = document.querySelector("dialog");
  const beforeClose = mutations();
  button("关闭", dialog).click();
  await wait();
  check(mutations() === beforeClose && document.querySelector("dialog[open]"), "点击关闭只打开确认表单");
  button("确认关闭 BUG", dialog).click();
  await wait();
  check(window.__bugCalls.at(-1).args.action === "close", "已解决 BUG 可关闭");
  document.querySelectorAll(".bugs-title")[2].click();
  await wait();
  dialog = document.querySelector("dialog");
  const beforeActivate = mutations();
  button("激活", dialog).click();
  await wait();
  check(mutations() === beforeActivate && document.querySelector("dialog[open]"), "点击激活只打开确认表单");
  button("确认激活 BUG", dialog).click();
  await wait();
  check(
    window.__bugCalls.at(-1).args.action === "activate" &&
      window.__bugCalls.at(-1).args.payload.assignedTo === "tester" &&
      !("openedBuild" in window.__bugCalls.at(-1).args.payload),
    "本人关闭后恢复原指派账号，默认沿用远端影响版本",
  );
  button("同步我的 BUG").click();
  await wait();
  check(
    window.__bugCalls.at(-1).name === "zentao_bugs_sync",
    "同步应调用独立 BUG IPC",
  );
  document.querySelectorAll(".bugs-status-filters button")[1].click();
  await wait();
  check(document.querySelectorAll("tbody tr").length === 1, "待解决筛选正常");
  document.documentElement.setAttribute("data-theme", "dark");
  return {
    states: true,
    safeHtml: true,
    images: true,
    imageRetry: true,
    imagePreview: true,
    unchangedSteps: true,
    comments: true,
    richTextEditing: true,
    editable: true,
    persistentError: true,
    resolve: true,
    close: true,
    activate: true,
    sync: true,
    filter: true,
  };
})();
