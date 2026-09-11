(async () => {
  const { native } = await import("/src/lib/api.ts");
  if (native) throw Error("仅浏览器预览可运行");
  const { mountExpiry } = await import("/tests/TaskExpiryFixture.tsx");
  const host = document.createElement("main");
  host.style.cssText =
    "position:fixed;inset:0;overflow:auto;background:var(--surface);z-index:9999";
  document.body.append(host);
  const dispose = mountExpiry(host);
  const wait = () => new Promise((r) => setTimeout(r, 400));
  await wait();
  const count = () =>
    [...host.querySelectorAll(".tag")].filter((e) => e.textContent === "已过期")
      .length;
  if (count() !== 3) throw Error("看板仅非关闭状态显示过期标签");
  const originalTheme = document.documentElement.getAttribute("data-theme");
  const colors = {};
  for (const theme of ["light", "dark"]) {
    document.documentElement.setAttribute("data-theme", theme);
    const badge = host.querySelector(".tag.expired");
    if (!badge) throw Error("过期标签应使用独立样式");
    const style = getComputedStyle(badge);
    colors[theme] = { color: style.color, background: style.backgroundColor };
    const [r, g, b] = style.color.match(/\d+/g).map(Number);
    if (!(r > g * 1.3 && r > b * 1.2)) throw Error(`${theme} 过期标签应为红色`);
  }
  if (originalTheme)
    document.documentElement.setAttribute("data-theme", originalTheme);
  else document.documentElement.removeAttribute("data-theme");
  [...host.querySelectorAll("button")]
    .find((b) => b.textContent.trim() === "列表")
    .click();
  await wait();
  if (count() !== 3) throw Error("列表仅非关闭状态显示过期标签");
  [...host.querySelectorAll("button")]
    .find((b) => b.textContent.trim() === "过期-closed")
    .click();
  await wait();
  if (
    [...document.querySelectorAll("dialog .tag")].some(
      (e) => e.textContent === "已过期",
    )
  )
    throw Error("已关闭任务详情不应显示过期标签");
  const detailText = document.querySelector("dialog").textContent;
  if (
    detailText.includes("截止日期") ||
    !detailText.includes("开始日期") ||
    !detailText.includes("结束日期")
  )
    throw Error("任务详情应只保留开始与结束日期");
  dispose();
  const { mountRemoteEditor } = await import("/tests/TaskExpiryFixture.tsx");
  const disposeRemote = mountRemoteEditor(host);
  await wait();
  const dialog = document.querySelector("dialog");
  const title = [...dialog.querySelectorAll("input")].find(
    (e) => e.value === "禅道执行",
  );
  if (!title || title.disabled) throw Error("禅道执行名称应可编辑");
  if (dialog.querySelectorAll('input[type="date"]').length !== 2)
    throw Error("无排期的禅道执行也应可填写全天开始与结束日期");
  if (dialog.textContent.includes("不会回写禅道"))
    throw Error("详情不应保留只读说明");
  disposeRemote();
  host.remove();
  return {
    board: true,
    list: true,
    closedDetails: true,
    unifiedDates: true,
    remoteEditor: true,
    colors,
  };
})();
