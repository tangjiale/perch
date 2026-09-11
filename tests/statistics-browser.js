// Vite 页面内通过 agent-browser eval --stdin 执行；所有数据在隔离 iframe 中。
(async () => {
  const frame = document.createElement("iframe");
  frame.style.cssText =
    "position:fixed;left:-1800px;top:0;width:1440px;height:1000px;border:0";
  document.body.append(frame);
  const win = frame.contentWindow,
    doc = frame.contentDocument;
  for (const node of document.querySelectorAll('style,link[rel="stylesheet"]'))
    doc.head.append(node.cloneNode(true));
  doc.documentElement.dataset.theme = "light";
  doc.documentElement.style.colorScheme = "light";
  const shell = document.querySelector(".app-shell").cloneNode(true);
  const host = shell.querySelector("main");
  host.innerHTML = "";
  shell.querySelector(".preview-notice").textContent =
    "统计验收数据 · 不写入本地工作空间";
  doc.body.append(shell);
  win.testHost = host;
  const errors = [],
    requests = [],
    results = [];
  win.addEventListener("error", (e) => errors.push(e.message));
  win.addEventListener("unhandledrejection", (e) =>
    errors.push(String(e.reason)),
  );
  win.isTauri = true;
  win.__TAURI_INTERNALS__ = {
    invoke: async (name) => {
      requests.push(name);
      if (name === "mcp_list")
        return [
          { id: "mcp1", name: "文件工具", enabled: true, transport: "stdio" },
          {
            id: "mcp2",
            name: "公司接口",
            enabled: false,
            transport: "streamable-http",
          },
        ];
      throw Error(`不允许测试写操作：${name}`);
    },
  };
  const base = new Date();
  base.setHours(12, 0, 0, 0);
  const at = (offset) => {
    const d = new Date(base);
    d.setDate(d.getDate() - offset);
    return d.getTime();
  };
  const empty = Object.fromEntries(
    [
      "tasks",
      "projects",
      "apps",
      "categories",
      "providers",
      "models",
      "agents",
      "skills",
      "knowledge",
      "documents",
      "conversations",
      "messages",
      "connections",
    ].map((key) => [key, []]),
  );
  const fixture = structuredClone(empty);
  fixture.bugs = ["active", "resolved", "closed"].map((status, index) => ({
    id: `bug-${index}`,
    connectionId: "c1",
    remoteId: String(index + 1),
    title: `验收 BUG ${index + 1}`,
    steps: "",
    status,
    assignedTo: "me",
    severity: index + 1,
    priority: 1,
    productId: "1",
    productName: "测试产品",
    openedDate: new Date(at(index)).toISOString(),
    resolvedDate:
      status === "resolved" ? new Date(at(0)).toISOString() : undefined,
  }));
  fixture.projects = [
    "客户服务平台",
    "移动端体验优化",
    "内部工具升级",
    "数据分析与报表",
  ].map((name, i) => ({
    id: `p${i}`,
    name,
    description: "",
    status: "doing",
    source: i % 2 ? "zentao" : "local",
  }));
  fixture.tasks = Array.from({ length: 36 }, (_, i) => ({
    id: `t${i}`,
    title: `任务 ${i}`,
    status: ["todo", "doing", "done", "closed"][i % 4],
    source: i % 3 ? "local" : "zentao",
    priority: ["high", "normal", "low"][i % 3],
    projectId: `p${i % 4}`,
    createdAt: at(i % 30),
    completedAt: i % 4 === 2 ? at(i % 7) : null,
    sortOrder: i,
    notes: "",
  }));
  fixture.knowledge = ["产品资料库", "技术文档库", "团队规范"].map(
    (name, i) => ({ id: `k${i}`, name, description: "", modelId: "embedding" }),
  );
  fixture.documents = Array.from({ length: 18 }, (_, i) => ({
    id: `d${i}`,
    name: `文档${i}.${["pdf", "docx", "xlsx", "md", "pptx", "txt"][i % 6]}`,
    knowledgeId: `k${i % 3}`,
    size: 32000 + i * 1024,
    status: ["ready", "ready", "parsed", "failed"][i % 4],
    chunks: i % 4 < 2 ? 12 : 0,
  }));
  fixture.categories = ["协作沟通", "设计研发", "业务系统"].map((name, i) => ({
    id: `cat${i}`,
    name,
  }));
  fixture.apps = Array.from({ length: 12 }, (_, i) => ({
    id: `app${i}`,
    name: `应用${i}`,
    url: "https://example.test",
    description: "",
    categoryId: `cat${i % 3}`,
    favorite: i % 3 === 0,
  }));
  fixture.providers = [{ id: "provider", name: "模型服务", enabled: true }];
  fixture.models = ["chat", "vision", "embedding"].map((capability, i) => ({
    id: `model${i}`,
    providerId: "provider",
    name: capability,
    capability,
    enabled: true,
  }));
  fixture.agents = ["工作助手", "代码审查", "文档助理"].map((name, i) => ({
    id: `agent${i}`,
    name,
    enabled: i !== 2,
    skillIds: ["s1"],
  }));
  fixture.skills = [
    { id: "s1", name: "会议纪要", enabled: true },
    { id: "s2", name: "代码规范", enabled: false },
  ];
  fixture.conversations = Array.from({ length: 15 }, (_, i) => ({
    id: `c${i}`,
    title: `会话${i}`,
    agentId: `agent${i % 3}`,
    knowledgeId: i % 2 ? "k0" : undefined,
    createdAt: at(i % 12),
  }));
  fixture.messages = Array.from({ length: 90 }, (_, i) => ({
    id: `msg${i}`,
    conversationId: `c${i % 15}`,
    role: i % 2 ? "assistant" : "user",
    status: i % 11 === 0 ? "failed" : "completed",
    content: "",
    createdAt: at(i % 30),
  }));
  fixture.connections = [
    { id: "z1", name: "公司禅道", enabled: true, lastSync: at(0) },
  ];
  win.fixture = fixture;
  win.emptyFixture = empty;
  const script = doc.createElement("script");
  script.type = "module";
  script.textContent = `
    import RefreshRuntime from '${location.origin}/@react-refresh';
    RefreshRuntime.injectIntoGlobalHook(window); window.$RefreshReg$=()=>{}; window.$RefreshSig$=()=>type=>type; window.__vite_plugin_react_preamble_installed__=true;
    const {mountStatistics}=await import('${location.origin}/tests/StatisticsFixture.tsx');
    const fixture=mountStatistics(window.testHost);
    window.drawStats=fixture.draw; window.disposeStats=fixture.dispose;
    window.drawStats(window.fixture); window.ready=true;
  `;
  doc.body.append(script);
  const wait = async (predicate) => {
    for (let i = 0; i < 120; i++) {
      if (predicate()) return;
      await new Promise((r) => setTimeout(r, 50));
    }
    throw Error(
      "统计等待超时 " +
        JSON.stringify({ errors, text: doc.body.innerText.slice(-500) }),
    );
  };
  const assert = (name, value) => {
    if (!value) throw Error(name);
    results.push({ name, passed: true });
  };
  const chartCount = () =>
    [...doc.querySelectorAll(".statistics-canvas")].filter((host) =>
      host.querySelector("canvas"),
    ).length;
  const clickView = async (id) => {
    doc.getElementById(`statistics-tab-${id}`).click();
    await wait(
      () =>
        doc
          .getElementById(`statistics-tab-${id}`)
          .getAttribute("aria-selected") === "true",
    );
  };
  const pixels = () =>
    [...doc.querySelectorAll(".statistics-canvas")].every((host) =>
      [...host.querySelectorAll("canvas")].some((canvas) => {
        const data = canvas
          .getContext("2d")
          .getImageData(0, 0, canvas.width, canvas.height).data;
        let nonblank = 0;
        for (let i = 3; i < data.length; i += 64) if (data[i] > 0) nonblank++;
        return nonblank > 20;
      }),
    );
  try {
    await wait(() => win.ready && chartCount() === 5);
    await wait(pixels);
    assert("工作维度五张图表渲染非空", pixels());
    assert(
      "统计使用传入工作空间记录",
      doc.querySelector(".statistics-summary dd").textContent === "36",
    );
    doc.querySelector('button[aria-label="任务状态分布：查看数据表"]').click();
    await wait(() => doc.querySelector('[aria-label="任务状态分布"] tbody'));
    assert(
      "图表切换数据表数值一致",
      [...doc.querySelectorAll('[aria-label="任务状态分布"] tbody tr')].every(
        (tr) => tr.lastElementChild.textContent === "9",
      ),
    );
    doc.querySelector('button[aria-label="任务状态分布：显示图表"]').click();
    doc.querySelector('[aria-label="统计趋势范围"]').click();
    await wait(() => doc.querySelector("[role=option]"));
    [...doc.querySelectorAll("[role=option]")]
      .find((option) => option.textContent.includes("7"))
      .click();
    await wait(() =>
      doc
        .querySelector('[aria-label="任务变化趋势"] p')
        .textContent.includes("7"),
    );
    assert(
      "时间范围只影响趋势，存量不变",
      doc.querySelector(".statistics-summary dd").textContent === "36",
    );
    await clickView("bugs");
    await wait(() => chartCount() === 4);
    await wait(pixels);
    assert("BUG 四张图表渲染非空", pixels());
    assert(
      "BUG 指标独立于任务计数",
      doc
        .querySelector(".statistics-summary-strip")
        .textContent.includes("BUG 总数3") &&
        doc.querySelector(".statistics-summary dd").textContent === "36",
    );
    doc.querySelector('button[aria-label="BUG 状态分布：查看数据表"]').click();
    await wait(() => doc.querySelector('[aria-label="BUG 状态分布"] tbody'));
    assert(
      "BUG 状态数据表对应三种状态",
      [...doc.querySelectorAll('[aria-label="BUG 状态分布"] tbody tr')].every(
        (tr) => tr.lastElementChild.textContent === "1",
      ),
    );
    doc.querySelector('button[aria-label="BUG 状态分布：显示图表"]').click();
    doc
      .getElementById("statistics-tab-bugs")
      .dispatchEvent(
        new win.KeyboardEvent("keydown", { key: "End", bubbles: true }),
      );
    await wait(
      () =>
        doc
          .getElementById("statistics-tab-ai")
          .getAttribute("aria-selected") === "true",
    );
    assert(
      "增加 Tab 后 End 键仍指向最后一项",
      doc.activeElement.id === "statistics-tab-ai",
    );
    await clickView("resources");
    await wait(() => chartCount() === 4);
    await wait(pixels);
    assert("知识与应用四张图表正常", pixels());
    await clickView("ai");
    await wait(() => chartCount() === 6);
    await wait(pixels);
    assert(
      "AI 图表包含日历热力图",
      doc.querySelector('[aria-label="消息活跃日历"] canvas') && pixels(),
    );
    assert(
      "MCP 仅读取配置",
      requests.length > 0 && requests.every((name) => name === "mcp_list"),
    );
    doc.documentElement.dataset.theme = "dark";
    doc.documentElement.style.colorScheme = "dark";
    frame.style.width = "900px";
    frame.style.height = "700px";
    await new Promise((resolve) => setTimeout(resolve, 400));
    assert(
      "深色窄屏无横向溢出",
      doc.documentElement.scrollWidth <= 900 &&
        [...doc.querySelectorAll(".statistics-canvas")].every(
          (node) => node.getBoundingClientRect().right <= 900,
        ),
    );
    win.drawStats(empty);
    await wait(() => chartCount() === 0);
    assert(
      "空数据不伪造图形或百分比",
      doc.querySelector(".statistics-summary dd").textContent === "0" &&
        doc.querySelectorAll(".statistics-chart-empty").length > 0,
    );
    assert("没有页面异常", errors.length === 0);
    if (window.keepStatisticsFixture) {
      win.drawStats(fixture);
      await clickView("work");
      await wait(() => chartCount() === 5);
      frame.style.cssText =
        "position:fixed;inset:0;width:100%;height:100%;border:0;z-index:10000";
      doc.documentElement.dataset.theme = "light";
      doc.documentElement.style.colorScheme = "light";
      window.statisticsFixture = frame;
    }
    return results;
  } finally {
    if (!window.keepStatisticsFixture) {
      win.disposeStats?.();
      frame.remove();
    }
  }
})();
