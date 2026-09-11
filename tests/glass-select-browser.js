(async () => {
  const results = [];
  const wait = () => new Promise((resolve) => setTimeout(resolve, 1200));
  const assert = (name, condition) => {
    results.push({ name, passed: !!condition });
    if (!condition) throw Error(name);
  };
  const select = (label) =>
    document.querySelector(`[role=combobox][aria-label="${label}"]`);
  const choose = async (control, label) => {
    control.click();
    await wait();
    const option = [...document.querySelectorAll("[role=option]")].find(
      (node) => node.textContent === label,
    );
    assert(`${label} 菜单项存在`, option);
    option.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "Enter" }));
    await wait();
    assert(`${label} 已更新`, control.textContent.includes(label));
    assert("选择后菜单关闭", !document.querySelector("[role=listbox]"));
  };
  location.hash = "/chat";
  await wait();
  assert(
    "聊天使用两个玻璃选择器",
    document.querySelectorAll(".chat-layout .glass-select-trigger").length ===
      2,
  );
  await choose(select("选择知识库"), "不使用知识库");
  assert("选择空值后焦点返回", document.activeElement === select("选择知识库"));
  location.hash = "/tasks";
  await wait();
  await choose(select("状态筛选"), "正在做");
  await choose(select("状态筛选"), "全部状态");
  [...document.querySelectorAll("button")]
    .find((node) => node.textContent.includes("新建任务"))
    .click();
  await wait();
  const control = document.querySelector("dialog .glass-select-trigger");
  control.click();
  await wait();
  assert(
    "菜单位于原生弹框内部",
    !!document.querySelector("dialog [role=listbox]"),
  );
  assert(
    "菜单在可视区域",
    document.querySelector("[role=listbox]").getBoundingClientRect().bottom <=
      innerHeight,
  );
  document.querySelector("[role=option]").dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "Enter" }));
  await wait();
  assert("选择后父弹窗保持开启", document.querySelector("dialog")?.open);
  document.querySelector('dialog button[title="关闭"]').click();
  await wait();
  for (const route of [
    "/apps",
    "/settings?tab=models",
    "/settings?tab=agents",
    "/settings?tab=knowledge",
    "/settings?tab=mcp",
    "/chat",
  ]) {
    location.hash = route;
    await wait();
    assert(
      `${route} 无可见原生下拉`,
      [...document.querySelectorAll("select")].every(
        (node) => node.getBoundingClientRect().width <= 1,
      ),
    );
    assert(
      `${route} 无页面横向溢出`,
      document.documentElement.scrollWidth <= innerWidth,
    );
  }
  return results;
})();
