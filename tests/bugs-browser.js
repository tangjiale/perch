(async () => {
  if (location.pathname !== "/tests/bugs.html")
    throw Error("仅允许在隔离 BUG fixture 中执行");
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
  document.querySelector(".bugs-title").click();
  await wait();
  let dialog = document.querySelector("dialog");
  check(
    dialog &&
      !dialog.querySelector(
        ".bugs-steps img, .bugs-steps a, .bugs-steps script",
      ),
    "复现内容必须移除远端图片与危险内容",
  );
  check(!window.__unsafeBug, "复现内容不应执行脚本");
  button("编辑 BUG", dialog).click();
  await wait();
  check(!dialog.querySelector("input").disabled, "标题应可编辑");
  window.__bugFail = true;
  button("保存到禅道", dialog).click();
  await wait();
  check(
    dialog.querySelector('[role="alert"]').textContent.includes("权限不足"),
    "写入失败应保留错误和详情",
  );
  check(
    window.__bugCalls.at(-1).name === "zentao_bug_save",
    "保存调用 BUG 领域 IPC",
  );
  window.__bugFail = false;
  button("取消编辑", dialog).click();
  await wait();
  button("解决", dialog).click();
  await wait();
  button("确认解决 BUG", dialog).click();
  await wait();
  const resolve = window.__bugCalls.at(-1);
  check(
    resolve.name === "zentao_bug_transition" &&
      resolve.args.action === "resolve" &&
      resolve.args.payload.resolvedBuild === "trunk" &&
      resolve.args.revision === 1,
    "解决动作携带版本及修订号",
  );
  check(!document.querySelector("dialog"), "成功后关闭详情");
  document.querySelectorAll(".bugs-title")[1].click();
  await wait();
  dialog = document.querySelector("dialog");
  button("关闭", dialog).click();
  await wait();
  button("确认关闭 BUG", dialog).click();
  await wait();
  check(window.__bugCalls.at(-1).args.action === "close", "已解决 BUG 可关闭");
  document.querySelectorAll(".bugs-title")[2].click();
  await wait();
  dialog = document.querySelector("dialog");
  button("激活", dialog).click();
  await wait();
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
    editable: true,
    persistentError: true,
    resolve: true,
    close: true,
    activate: true,
    sync: true,
    filter: true,
  };
})();
