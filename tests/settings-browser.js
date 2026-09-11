(async () => {
  const results = [];
  const wait = () => new Promise((resolve) => setTimeout(resolve, 1200));
  const assert = (name, condition) => {
    results.push({ name, passed: !!condition });
    if (!condition) throw Error(name);
  };
  for (const [route, tab, heading] of [
    ["agents", "agents", "Agent"],
    ["knowledge", "knowledge", "知识库"],
    ["skills", "skills", "技能"],
  ]) {
    location.hash = `/${route}`;
    await wait();
    assert(
      `${heading} 旧地址跳转到设置`,
      location.hash === `#/settings?tab=${tab}`,
    );
    assert(
      `${heading} 设置内容存在`,
      document.querySelector(".settings-panel h2")?.textContent === heading,
    );
    assert(
      `${heading} 仅有一个页面主标题`,
      document.querySelectorAll("main h1").length === 1,
    );
  }
  assert(
    "左侧不再有三个独立菜单",
    ![...document.querySelectorAll(".sidebar nav a")].some((a) =>
      ["Agent", "技能", "知识库"].includes(a.textContent),
    ),
  );
  location.hash = "/settings?tab=mcp";
  await wait();
  assert("九个设置 Tab", document.querySelectorAll("[role=tab]").length === 9);
  assert(
    "MCP Tab 选中",
    document
      .querySelector("#settings-tab-mcp")
      .getAttribute("aria-selected") === "true",
  );
  [...document.querySelectorAll(".settings-panel button")]
    .find((b) => b.textContent === "添加连接")
    .click();
  await wait();
  assert(
    "MCP 表单默认 HTTP",
    !!document.querySelector("dialog input[type=url]"),
  );
  document.querySelector("dialog [role=combobox]").click();
  await wait();
  [...document.querySelectorAll("[role=option]")]
    .find((option) => option.textContent === "STDIO")
    .dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "Enter" }));
  await wait();
  assert(
    "STDIO 命令与参数表单",
    document.querySelector("dialog").textContent.includes("启动参数") &&
      !document.querySelector("dialog input[type=url]"),
  );
  assert(
    "STDIO 环境变量配置",
    document.querySelector("dialog").textContent.includes("环境变量"),
  );
  [...document.querySelectorAll("dialog button")]
    .find((b) => b.textContent === "取消")
    .click();
  await wait();
  assert("关闭表单", !document.querySelector("dialog"));
  assert(
    "页面没有横向溢出",
    document.documentElement.scrollWidth <= innerWidth,
  );
  return results;
})();
