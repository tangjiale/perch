(async () => {
  const results = [];
  const wait = () => new Promise((resolve) => setTimeout(resolve, 1000));
  const assert = (name, condition) => {
    results.push({ name, passed: !!condition });
    if (!condition) throw Error(name);
  };
  location.hash = "/settings?tab=general";
  await wait();
  const tabs = [...document.querySelectorAll("[role=tab]")];
  assert(
    "通用设置位于关于前",
    tabs.at(-2).textContent === "通用设置" &&
      tabs.at(-1).textContent === "关于",
  );
  document.querySelector("input[name=appearance][value=dark]").click();
  await wait();
  assert("深色即时应用", document.documentElement.dataset.theme === "dark");
  assert(
    "深色偏好已保存",
    JSON.parse(localStorage.getItem("perch.appearance.v1")).mode === "dark",
  );
  const { emptySnapshot } = await import("/src/lib/api.ts");
  const { saveAppearance } = await import("/src/lib/appearance.ts");
  const day = new Date().toLocaleDateString("en-CA");
  emptySnapshot.tasks = [
    {
      id: "theme-task",
      title: "界面测试 · 设计评审",
      notes: "",
      source: "local",
      status: "doing",
      priority: "normal",
      sortOrder: 1,
      schedule: {
        kind: "timed",
        timezone: "Asia/Shanghai",
        start: `${day}T09:00:00+08:00`,
        end: `${day}T10:00:00+08:00`,
      },
    },
  ];
  emptySnapshot.agents = [
    {
      id: "theme-agent",
      name: "界面测试助手",
      description: "",
      modelId: "",
      enabled: true,
      skillIds: [],
    },
  ];
  emptySnapshot.conversations = [
    { id: "theme-chat", title: "界面测试会话", agentId: "theme-agent" },
  ];
  emptySnapshot.messages = [
    {
      id: "theme-message",
      conversationId: "theme-chat",
      role: "assistant",
      status: "completed",
      content:
        "这是一段界面测试内容。\n\n```text\n深色代码块\n```\n\n| 项目 | 状态 |\n| --- | --- |\n| 设计评审 | 正在做 |",
    },
  ];
  for (const route of [
    "/",
    "/projects",
    "/tasks",
    "/calendar",
    "/apps",
    "/chat",
    "/settings?tab=models",
    "/settings?tab=knowledge",
    "/settings?tab=mcp",
  ]) {
    location.hash = route;
    await wait();
    assert(
      `${route} 深色保持`,
      document.documentElement.dataset.theme === "dark",
    );
    assert(
      `${route} 无横向溢出`,
      document.documentElement.scrollWidth <= innerWidth,
    );
  }
  location.hash = "/chat";
  await wait();
  saveAppearance({ mode: "dark", chatFontSize: 20 });
  await wait();
  assert(
    "会话字号调整到20px",
    getComputedStyle(document.querySelector(".message")).fontSize === "20px",
  );
  assert(
    "输入框字号保持14px",
    getComputedStyle(document.querySelector(".chat-composer textarea"))
      .fontSize === "14px",
  );
  assert(
    "代码块随会话字号缩放",
    parseFloat(
      getComputedStyle(document.querySelector(".message pre")).fontSize,
    ) > 16,
  );
  location.hash = "/settings?tab=mcp";
  await wait();
  document.querySelector(".section-heading button").click();
  await wait();
  const modal = document.querySelector("dialog");
  assert(
    "弹窗使用深色背景",
    parseFloat(getComputedStyle(modal).backgroundColor.match(/[\d.]+/)[0]) < 80,
  );
  modal.querySelector("[role=combobox]").click();
  await wait();
  assert(
    "下拉菜单使用深色背景",
    parseFloat(
      getComputedStyle(
        document.querySelector("[role=listbox]"),
      ).backgroundColor.match(/[\d.]+/)[0],
    ) < 80,
  );
  document
    .querySelector("[role=option]")
    .dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
  await wait();
  modal.querySelector("button[title=关闭]").click();
  await wait();
  location.hash = "/settings?tab=general";
  await wait();
  document.querySelector("input[name=appearance][value=light]").click();
  await wait();
  assert("返回浅色正常", document.documentElement.dataset.theme === "light");
  document.querySelector("input[name=appearance][value=dark]").click();
  await wait();
  return results;
})();
