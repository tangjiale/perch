// 在 Vite 页面中通过 agent-browser eval --stdin 执行，仅使用隔离 iframe 和模拟 IPC。
(async () => {
  const frame = document.createElement("iframe");
  frame.style.cssText =
    "position:fixed;inset:0;width:100vw;height:100vh;border:0;z-index:99999";
  document.body.append(frame);
  const win = frame.contentWindow,
    doc = frame.contentDocument,
    requests = [],
    results = [];
  for (const node of document.querySelectorAll('style,link[rel="stylesheet"]'))
    doc.head.append(node.cloneNode(true));
  doc.documentElement.dataset.theme = "light";
  win.isTauri = true;
  win.__TAURI_INTERNALS__ = {
    transformCallback: () => 1,
    unregisterCallback: () => {},
    invoke: (name, args) => {
      requests.push({ name, args });
      if (name === "chat_send" && win.failChat)
        return Promise.reject("模拟发送失败");
      return Promise.resolve(
        name === "save_conversation" ? args.value : undefined,
      );
    },
  };
  const script = doc.createElement("script");
  script.type = "module";
  script.textContent = `
    import RefreshRuntime from '${location.origin}/@react-refresh';
    RefreshRuntime.injectIntoGlobalHook(window);
    window.$RefreshReg$ = () => {}; window.$RefreshSig$ = () => type => type;
    window.__vite_plugin_react_preamble_installed__ = true;
    const { mountChat } = await import('${location.origin}/tests/ChatImagesFixture.tsx');
    const host = document.createElement('main'); document.body.append(host);
    window.dispose = mountChat(host, true);
  `;
  doc.body.append(script);
  const wait = async (predicate) => {
    for (let i = 0; i < 150; i++) {
      if (predicate()) return;
      await new Promise((r) => setTimeout(r, 30));
    }
    throw Error("等待超时：" + doc.body.innerText);
  };
  const assert = (name, condition) => {
    if (!condition) throw Error(name);
    results.push(name);
  };
  const button = (label) =>
    [...doc.querySelectorAll("button")].find(
      (b) => b.textContent.trim() === label,
    );
  try {
    await wait(() => doc.querySelector("textarea"));
    const textarea = doc.querySelector("textarea");
    assert("无会话时选择 Agent 即可输入", !textarea.disabled);
    const fill = (value) => {
      Object.getOwnPropertyDescriptor(
        win.HTMLTextAreaElement.prototype,
        "value",
      ).set.call(textarea, value);
      textarea.dispatchEvent(new win.Event("input", { bubbles: true }));
    };
    fill("第一条工作消息");
    await wait(() => !button("发送").disabled);
    win.failChat = true;
    button("发送").click();
    button("发送")?.click();
    await wait(() => win.lastNotice === "模拟发送失败" && !button("停止"));
    assert("首次发送失败保留输入", textarea.value === "第一条工作消息");
    const first = requests.filter((r) => r.name === "chat_send");
    assert(
      "首次发送自动创建且只发送一次",
      first.length === 1 &&
        requests.some((r) => r.name === "save_conversation"),
    );
    const id = first[0].args.conversationId;
    assert(
      "新建会话绑定选中的 Agent",
      requests.find((r) => r.name === "save_conversation").args.value
        .agentId === "vision",
    );
    win.failChat = false;
    button("发送").click();
    await wait(
      () =>
        requests.filter((r) => r.name === "chat_send").length === 2 &&
        !button("停止"),
    );
    assert(
      "失败重试复用会话而非重复创建",
      requests
        .filter((r) => r.name === "save_conversation")
        .every((r) => r.args.value.id === id),
    );
    assert("成功发送后清空输入", textarea.value === "");
    return results;
  } finally {
    if (!window.keepChatFixture) {
      win.dispose?.();
      frame.remove();
    }
  }
})();
