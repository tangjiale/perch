// 在 Vite 浏览器预览内通过 agent-browser eval --stdin 运行；仅模拟 IPC。
(async () => {
  const frame = document.createElement("iframe");
  frame.style.cssText = "position:fixed;left:-1600px;width:900px;height:800px";
  document.body.append(frame);
  const win = frame.contentWindow,
    doc = frame.contentDocument;
  const errors = [],
    requests = [],
    results = [];
  win.addEventListener("error", (e) => errors.push(e.message));
  win.addEventListener("unhandledrejection", (e) =>
    errors.push(String(e.reason)),
  );
  win.isTauri = true;
  let configured = true,
    checkError = false,
    current = false,
    blocked = false;
  let download;
  win.__TAURI_INTERNALS__ = {
    transformCallback: () => 1,
    unregisterCallback: () => {},
    invoke: async (command, args) => {
      requests.push({ command, args });
      if (command === "updater_status")
        return {
          configured,
          releasesUrl: "https://github.com/example/perch/releases/latest",
        };
      if (command === "update_prepare") {
        if (blocked) throw Error("会话进行中");
        return;
      }
      if (command === "plugin:updater|check") {
        if (checkError) throw Error("网络错误");
        return current
          ? null
          : {
              rid: 10,
              currentVersion: "0.1.0",
              version: "0.1.1",
              body: "新增：在线更新\n修复：日历跨日显示",
              rawJson: {},
            };
      }
      if (command === "plugin:updater|download_and_install")
        return new Promise((resolve, reject) => {
          download = { resolve, reject, channel: args.onEvent };
        });
      if (
        [
          "plugin:resources|close",
          "plugin:process|restart",
          "plugin:opener|open_url",
        ].includes(command)
      )
        return;
      throw Error(`意外命令：${command}`);
    },
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
    const { default: Updater } = await import('${location.origin}/src/components/UpdatePopover.tsx');
    const host = document.createElement('div'); document.body.append(host);
    const root = createRoot(host); let key = 0;
    window.resetUpdater = () => root.render(React.createElement(Updater, {key: ++key, version: '0.1.0'}));
    window.resetUpdater(); window.ready = true;
  `;
  doc.body.append(script);
  const wait = async (predicate) => {
    for (let i = 0; i < 100; i++) {
      if (predicate()) return;
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    throw Error(
      "等待更新组件超时：" +
        JSON.stringify({ errors, text: doc.body.innerText }),
    );
  };
  const assert = (name, value) => {
    if (!value) throw Error(name);
    results.push({ name, passed: true });
  };
  const findButton = (text) =>
    [...doc.querySelectorAll("button")].find((button) =>
      button.textContent.includes(text),
    );
  const primary = () => doc.querySelector(".update-primary-action");
  const status = () => doc.querySelector(".update-status")?.textContent || "";
  const open = async () => {
    await wait(() => doc.querySelector(".update-version-trigger"));
    doc.querySelector(".update-version-trigger").click();
    await wait(() => doc.querySelector(".update-popover"));
  };
  const reset = async () => {
    win.resetUpdater();
    await wait(() => !doc.querySelector(".update-popover"));
    await open();
  };
  try {
    await wait(() => win.ready);
    await open();
    await wait(() => status().includes("有新版本"));
    assert(
      "检查禁止降级并设置超时",
      requests.find((r) => r.command === "plugin:updater|check").args
        .allowDowngrades === false,
    );
    assert(
      "新版本显示标记与多条中文日志",
      doc.querySelector(".update-version-dot") &&
        doc.querySelector(".update-notes").textContent.includes("日历"),
    );
    findButton("查看更新日志").click();
    await wait(() =>
      requests.some((r) => r.command === "plugin:opener|open_url"),
    );
    blocked = true;
    primary().click();
    await wait(() => doc.querySelector(".update-error"));
    assert(
      "活动会话阻止安装且可重试",
      !download && primary().textContent.includes("重试"),
    );
    blocked = false;
    primary().click();
    await wait(() => download);
    download.channel.onmessage({
      event: "Started",
      data: { contentLength: 100 },
    });
    download.channel.onmessage({
      event: "Progress",
      data: { chunkLength: 40 },
    });
    await wait(() => doc.querySelector("progress")?.value === 40);
    assert("显示真实事件进度且阻止重复安装", primary().disabled);
    doc.querySelector(".update-version-trigger").click();
    await wait(() => !doc.querySelector(".update-popover"));
    await open();
    assert(
      "关闭弹层后继续保留下载进度",
      doc.querySelector("progress")?.value === 40,
    );
    download.reject(Error("连接断开"));
    await wait(() => primary().textContent.includes("重试更新"));
    download = null;
    primary().click();
    await wait(() => download);
    download.channel.onmessage({ event: "Started", data: {} });
    await wait(
      () =>
        doc.querySelector("progress") &&
        !doc.querySelector("progress").hasAttribute("value"),
    );
    assert(
      "未知文件大小使用不定进度",
      doc.querySelector(".update-progress").textContent.includes("已下载"),
    );
    download.channel.onmessage({ event: "Finished" });
    download.resolve();
    await wait(() => primary().textContent.includes("重启应用"));
    assert(
      "安装完成释放原生资源",
      requests.some(
        (r) => r.command === "plugin:resources|close" && r.args.rid === 10,
      ),
    );
    primary().click();
    await wait(() =>
      requests.some((r) => r.command === "plugin:process|restart"),
    );
    assert(
      "重启前再次检查活动会话",
      requests.filter((r) => r.command === "update_prepare").length === 4,
    );
    checkError = true;
    await reset();
    await wait(() => status().includes("暂未完成"));
    assert("检查失败支持重新检查", primary().textContent.includes("重新检查"));
    checkError = false;
    current = true;
    primary().click();
    await wait(() => status().includes("已是最新"));
    assert(
      "已是最新版本不显示升级按钮",
      !doc.querySelector(".update-version-dot") &&
        primary().textContent.includes("检查更新"),
    );
    configured = false;
    const checks = requests.filter(
      (r) => r.command === "plugin:updater|check",
    ).length;
    await reset();
    await wait(() => status().includes("尚未配置"));
    assert(
      "未配置更新源不发网络检查",
      primary().disabled &&
        requests.filter((r) => r.command === "plugin:updater|check").length ===
          checks,
    );
    assert("无浏览器异常", errors.length === 0);
    return results;
  } finally {
    frame.remove();
  }
})();
