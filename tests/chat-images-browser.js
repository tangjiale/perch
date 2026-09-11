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
    window.dispose = mountChat(host);
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
  const png =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a/6kAAAAASUVORK5CYII=";
  const file = () =>
    new win.File(
      [Uint8Array.from(atob(png), (c) => c.charCodeAt(0))],
      "测试图片.png",
      { type: "image/png" },
    );
  const upload = (files) => {
    const transfer = new win.DataTransfer();
    files.forEach((f) => transfer.items.add(f));
    const input = doc.querySelector('input[type="file"]');
    input.files = transfer.files;
    input.dispatchEvent(new win.Event("change", { bubbles: true }));
  };
  try {
    await wait(() => button("添加图片"));
    assert("视觉会话可添加图片", !button("添加图片").disabled);
    upload([file()]);
    await wait(() =>
      doc.querySelector(".chat-composer .chat-image-gallery img"),
    );
    assert("纯图片可发送", !button("发送").disabled);
    doc.querySelector('[aria-label="移除图片 测试图片.png"]').click();
    await wait(() => !doc.querySelector(".chat-composer .chat-image-gallery"));
    assert("移除最后图片后不能发送空消息", button("发送").disabled);
    const transfer = new win.DataTransfer();
    transfer.items.add(file());
    doc
      .querySelector("textarea")
      .dispatchEvent(
        new win.ClipboardEvent("paste", {
          clipboardData: transfer,
          bubbles: true,
        }),
      );
    await wait(() =>
      doc.querySelector(".chat-composer .chat-image-gallery img"),
    );
    button("发送").click();
    await wait(
      () => requests.some((r) => r.name === "chat_send") && !button("停止"),
    );
    const sent = requests.find((r) => r.name === "chat_send").args;
    assert(
      "图片通过 IPC 发送且支持纯图",
      sent.content === "" &&
        sent.images.length === 1 &&
        sent.images[0].dataUrl.startsWith("data:image/png;base64,"),
    );
    win.updateChatFixture([
      {
        id: "message",
        conversationId: "vision",
        role: "user",
        content: "",
        status: "completed",
        images: sent.images,
      },
    ]);
    await wait(() => doc.querySelector(".messages .chat-image-gallery img"));
    assert(
      "刷新历史消息仍显示图片",
      !!doc.querySelector(".messages .chat-image-gallery img"),
    );
    win.failChat = true;
    upload([file()]);
    await wait(() =>
      doc.querySelector(".chat-composer .chat-image-gallery img"),
    );
    button("发送").click();
    await wait(() => win.lastNotice === "模拟发送失败" && !button("停止"));
    assert(
      "发送失败保留图片供重试",
      !!doc.querySelector(".chat-composer .chat-image-gallery img"),
    );
    [...doc.querySelectorAll(".conversation-list button")].find(b => b.textContent.trim() === "文本助手").click();
    await wait(() => button("添加图片").disabled);
    assert(
      "文本会话禁用图片并隔离附件草稿",
      !doc.querySelector(".chat-composer .chat-image-gallery"),
    );
    [...doc.querySelectorAll(".conversation-list button")].find(b => b.textContent.trim() === "视觉助手").click();
    await wait(() => !button("添加图片").disabled);
    upload([file(), file(), file(), file(), file()]);
    await wait(() => win.lastNotice?.includes("最多添加 4"));
    assert(
      "超数量显示中文提示",
      !doc.querySelector(".chat-composer .chat-image-gallery"),
    );
    upload([file()]);
    await wait(() =>
      doc.querySelector(".chat-composer .chat-image-gallery img"),
    );
    if (window.keepChatFixture) {
      win.chatResults = results;
      return results;
    }
    return results;
  } finally {
    if (!window.keepChatFixture) {
      win.dispose?.();
      frame.remove();
    }
  }
})();
