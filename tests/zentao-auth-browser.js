// 在 Vite 页面中通过 agent-browser eval --stdin 运行，不调用真实禅道。
(async () => {
  const frame = document.createElement("iframe");
  frame.style.cssText = "position:fixed;left:-1500px;width:900px;height:900px";
  document.body.append(frame);
  const win = frame.contentWindow,
    doc = frame.contentDocument;
  for (const node of document.querySelectorAll('style,link[rel="stylesheet"]'))
    doc.head.append(node.cloneNode(true));
  const requests = [],
    errors = [],
    results = [];
  win.isTauri = true;
  win.refreshFails = true;
  win.closeCount = 0;
  win.addEventListener("error", (e) => errors.push(e.message));
  win.addEventListener("unhandledrejection", (e) =>
    errors.push(String(e.reason)),
  );
  win.__TAURI_INTERNALS__ = {
    invoke: (name, args) =>
      name === "zentao_auth_preferences"
        ? Promise.resolve(win.preferences)
        : new Promise((resolve, reject) =>
            requests.push({ name, args, resolve, reject }),
          ),
  };
  const script = doc.createElement("script");
  script.type = "module";
  script.textContent = `
    import RefreshRuntime from '${location.origin}/@react-refresh';
    RefreshRuntime.injectIntoGlobalHook(window);
    window.$RefreshReg$ = () => {}; window.$RefreshSig$ = () => type => type;
    window.__vite_plugin_react_preamble_installed__ = true;
    const reactModule = await import('${location.origin}/node_modules/.vite/deps/react.js');
    const React = reactModule.default ?? reactModule;
    const domModule = await import('${location.origin}/node_modules/.vite/deps/react-dom_client.js');
    const { createRoot } = domModule.default ?? domModule;
    const { default: Dialog } = await import('${location.origin}/src/components/ZentaoConnectionDialog.tsx');
    const host = document.createElement('div'); document.body.append(host);
    const root = createRoot(host); let key = 0;
    window.renderDialog = (existing, remembered = false, inline = false) => { window.preferences = {hasSavedPassword:remembered,allowInsecureHttp:remembered}; root.render(React.createElement(Dialog, {
      inline,
      key: ++key, workspaceId: 'fixture-workspace',
      connection: {id:'fixture-connection', name:'公司禅道', baseUrl:'http://example.test/zentao', apiVersion:'v2', enabled:true, managementEnabled:false, rememberCredentials:remembered, loginAccount:remembered?'fixture-user':'', ...(existing ? {revision:1} : {})},
      onClose: () => {window.closeCount++; root.render(null)},
      onSaved: async () => {if(window.refreshFails) throw Error('模拟刷新失败')}
    })); };
    window.renderDialog(false); window.ready = true;
  `;
  doc.body.append(script);
  const wait = async (predicate) => {
    for (let i = 0; i < 100; i++) {
      if (predicate()) return;
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    throw Error(
      "等待超时 " + JSON.stringify({ errors, text: doc.body.innerText }),
    );
  };
  const assert = (name, value) => {
    if (!value) throw Error(name);
    results.push({ name, passed: true });
  };
  const field = (text) =>
    [...doc.querySelectorAll("label")]
      .find((label) => label.textContent.trim().startsWith(text))
      ?.querySelector("input");
  const fill = (input, value) => {
    Object.getOwnPropertyDescriptor(
      win.HTMLInputElement.prototype,
      "value",
    ).set.call(input, value);
    input.dispatchEvent(new win.Event("input", { bubbles: true }));
  };
  const submit = () => doc.querySelector("footer button");
  const mode = (text) =>
    [...doc.querySelectorAll(".segmented button")].find((b) =>
      b.textContent.includes(text),
    );
  try {
    await wait(() => win.ready && field("密码"));
    assert("默认每五分钟自动同步", doc.querySelector('[aria-label="禅道自动同步频率"]').textContent.includes("每 5 分钟"));
    assert(
      "新建默认账号方式",
      mode("账号登录").getAttribute("aria-pressed") === "true",
    );
    fill(field("禅道账号"), "fixture-user");
    fill(field("密码"), "fixture-password");
    await wait(() => field("密码").value === "fixture-password");
    assert("HTTP 未授权时不能提交", submit().disabled);
    assert("默认不记住密码", !field("记住登录凭据").checked);
    field("记住登录凭据").click();
    field("我允许通过").click();
    await wait(() => !submit().disabled);
    submit().click();
    submit().click();
    await wait(() => requests.length === 1);
    assert(
      "只有一次登录命令且元数据不含密码",
      requests[0].name === "zentao_connect" &&
        requests[0].args.connection.syncIntervalMinutes === 5 &&
        requests[0].args.password === "fixture-password" &&
        !("password" in requests[0].args.connection),
    );
    assert(
      "业务 v2 不改变账号登录参数",
      requests[0].args.connection.apiVersion === "v2" &&
        requests[0].args.allowInsecureHttp === true &&
        requests[0].args.rememberCredentials === true &&
        requests[0].args.token === null,
    );
    const cancel = new win.Event("cancel", { cancelable: true });
    doc.querySelector("dialog").dispatchEvent(cancel);
    doc.querySelector('button[title="关闭"]').click();
    assert(
      "忙时拒绝关闭和 Escape",
      win.closeCount === 0 &&
        cancel.defaultPrevented &&
        doc.querySelector("dialog").open,
    );
    requests[0].resolve({
      id: "fixture-connection",
      revision: 1,
      hasCredential: true,
    });
    await wait(() => submit().textContent.includes("重试刷新"));
    assert("保存成功立即清空密码", field("密码").value === "");
    win.refreshFails = false;
    submit().click();
    await wait(() => win.closeCount === 1);
    assert("刷新重试不会再次登录", requests.length === 1);
    win.renderDialog(true);
    await wait(() => field("访问令牌"));
    assert(
      "已有连接保留令牌且地址只读",
      field("服务地址").readOnly && !field("访问令牌").required,
    );
    submit().click();
    await wait(() => requests.length === 2);
    assert(
      "保留令牌不发送账号密码",
      requests[1].args.token === null &&
        requests[1].args.account === null &&
        requests[1].args.password === null,
    );
    requests[1].reject("模拟连接保存失败");
    await wait(() => doc.querySelector("[role=alert]"));
    assert(
      "失败保留弹框和重试入口",
      doc.querySelector("dialog").open && !submit().disabled,
    );
    mode("账号登录").click();
    await wait(() => field("密码"));
    fill(field("密码"), "temporary-password");
    mode("访问令牌").click();
    await wait(() => field("访问令牌"));
    mode("账号登录").click();
    await wait(() => field("密码"));
    assert("切换模式清空密码", field("密码").value === "");
    win.renderDialog(true, true);
    await wait(() => field("密码")?.readOnly && field("记住登录凭据"));
    assert(
      "重新打开恢复账号登录及HTTP选项",
      mode("账号登录").getAttribute("aria-pressed") === "true" &&
        field("我允许通过").checked &&
        field("禅道账号").value === "fixture-user",
    );
    assert(
      "已保存密码仅用固定掩码且不可直接查看",
      field("密码").value === "********" &&
        field("密码").type === "password" &&
        field("密码").readOnly,
    );
    assert("已有自动登录选项保持开启", field("记住登录凭据").checked);
    submit().click();
    await wait(() => requests.length === 3);
    assert(
      "普通编辑保留记住凭据且不读取密码",
      requests[2].args.rememberCredentials === true &&
        requests[2].args.password === null,
    );
    requests[2].reject("模拟保存失败");
    await wait(() => !submit().disabled);
    field("记住登录凭据").click();
    submit().click();
    await wait(() => requests.length === 4);
    assert("可取消记住凭据", requests[3].args.rememberCredentials === false);
    requests[3].resolve({
      id: "fixture-connection",
      revision: 2,
      rememberCredentials: false,
    });
    await wait(() => !doc.querySelector("dialog"));
    assert("没有浏览器异常", errors.length === 0);
    win.renderDialog(true, true, true);
    await wait(
      () => doc.querySelector(".zentao-inline-form") && field("密码")?.readOnly,
    );
    assert(
      "内嵌账号和密码输入框对齐",
      Math.abs(
        field("密码").getBoundingClientRect().top -
          field("禅道账号").getBoundingClientRect().top,
      ) < 1,
    );
    [...doc.querySelectorAll("button")]
      .find((button) => button.textContent === "修改密码")
      .click();
    await wait(() => !field("密码").readOnly);
    assert(
      "主动修改时清空掩码且仍隐藏输入",
      field("密码").value === "" && field("密码").type === "password",
    );
    fill(field("密码"), "replacement-password");
    [...doc.querySelectorAll("button")]
      .find((button) => button.textContent === "保留原密码")
      .click();
    await wait(() => field("密码").readOnly);
    assert(
      "取消修改保留原凭据且未提交密码",
      field("密码").value === "********" && requests.length === 4,
    );
    return results;
  } finally {
    frame.remove();
  }
})();
