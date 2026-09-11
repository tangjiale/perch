(async () => {
  const frame = document.createElement("iframe");
  frame.style.cssText = "position:fixed;left:-1000px;width:500px;height:300px";
  document.body.append(frame);
  const win = frame.contentWindow;
  const doc = frame.contentDocument;
  win.testErrors = [];
  win.addEventListener("error", (event) => win.testErrors.push(event.message));
  win.addEventListener("unhandledrejection", (event) =>
    win.testErrors.push(String(event.reason)),
  );
  const png = await (await fetch("/src-tauri/icons/32x32.png")).blob();
  const dataUrl = await new Promise((resolve) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.readAsDataURL(png);
  });
  win.iconFixture = dataUrl;
  win.isTauri = true;
  win.requests = [];
  win.__TAURI_INTERNALS__ = {
    invoke: (command, args) =>
      new Promise((resolve, reject) =>
        win.requests.push({ command, args, resolve, reject }),
      ),
  };
  const script = doc.createElement("script");
  script.type = "module";
  script.textContent = `
    import RefreshRuntime from '${location.origin}/@react-refresh';
    RefreshRuntime.injectIntoGlobalHook(window);
    window.$RefreshReg$ = () => {};
    window.$RefreshSig$ = () => type => type;
    window.__vite_plugin_react_preamble_installed__ = true;
    const reactModule = await import('${location.origin}/node_modules/.vite/deps/react.js');
    const React = reactModule.default ?? reactModule;
    const domModule = await import('${location.origin}/node_modules/.vite/deps/react-dom_client.js');
    const { createRoot } = domModule.default ?? domModule;
    const { default: Picker } = await import('${location.origin}/src/components/AppLogoPicker.tsx');
    const host = document.createElement('div'); document.body.append(host);
    const root = createRoot(host);
    let url = 'https://example.test/', value = '', key = 0;
    const render = () => root.render(React.createElement(Picker, {key,url,value,onChange: logo => { value = logo; window.logo = logo; render(); }}));
    window.setUrl = next => { url = next; render(); };
    window.resetPicker = () => { key++; value = ''; window.logo = ''; render(); };
    render(); window.ready = true;
  `;
  doc.body.append(script);
  const wait = async (predicate) => {
    for (let i = 0; i < 80; i++) {
      if (predicate()) return;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw Error(
      "组件等待超时 " +
        JSON.stringify({
          ready: win.ready,
          requests: win.requests.length,
          errors: win.testErrors,
        }),
    );
  };
  const results = [];
  const assert = (name, condition) => {
    results.push({ name, passed: !!condition });
    if (!condition) throw Error(name);
  };
  try {
    await wait(() => win.ready && win.requests.length === 1);
    assert(
      "自动调用网站图标命令",
      win.requests[0].command === "website_icon_fetch",
    );
    win.requests[0].resolve(dataUrl);
    await wait(() => win.logo === dataUrl);
    assert(
      "成功显示图标和状态",
      doc.querySelector(".logo-file-status").textContent === "已获取网站图标",
    );
    win.setUrl("https://another.test/");
    await wait(() => win.requests.length === 2);
    assert("网址变化清除旧自动图标", win.logo === "");
    const transfer = new win.DataTransfer();
    transfer.items.add(
      new win.File([png], "手动图片.png", { type: "image/png" }),
    );
    const input = doc.querySelector("input[type=file]");
    input.files = transfer.files;
    input.dispatchEvent(new win.Event("change", { bubbles: true }));
    await wait(
      () =>
        doc.querySelector(".logo-file-status").textContent === "手动图片.png",
    );
    win.requests[1].resolve(dataUrl);
    await new Promise((resolve) => setTimeout(resolve, 300));
    assert(
      "迟到自动结果不覆盖手动图片",
      doc.querySelector(".logo-file-status").textContent === "手动图片.png",
    );
    win.setUrl("https://third.test/");
    await new Promise((resolve) => setTimeout(resolve, 1100));
    assert("手动图片不随网址变化重新获取", win.requests.length === 2);
    win.resetPicker();
    await wait(() => win.requests.length === 3);
    win.requests[2].reject("not found");
    await wait(() =>
      doc.querySelector(".logo-file-status").textContent.includes("未获取到"),
    );
    assert(
      "失败后仍可手动上传",
      !doc.querySelector(".logo-file-control button").disabled,
    );
    assert(
      "上传入口没有默认英文文案",
      !/Choose File|no file selected/.test(doc.body.innerText),
    );
    return results;
  } finally {
    frame.remove();
  }
})();
