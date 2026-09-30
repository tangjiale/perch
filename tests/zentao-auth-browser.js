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
    window.renderDialog = (existing, remembered = false, inline = false, hasCredential = existing) => { window.preferences = {hasSavedPassword:remembered,allowInsecureHttp:remembered}; root.render(React.createElement(Dialog, {
      inline,
      key: ++key, workspaceId: 'fixture-workspace',
      connection: {id:'fixture-connection', name:'公司禅道', baseUrl:'http://example.test/zentao', apiVersion:'v2', enabled:true, managementEnabled:false, hasCredential, rememberCredentials:remembered, loginAccount:remembered?'fixture-user':'', ...(existing ? {revision:1} : {})},
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
      "已有连接可编辑地址且原地址允许保留令牌",
      !field("服务地址").readOnly && !field("访问令牌").required,
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
    fill(field("服务地址"), "HTTP://EXAMPLE.TEST:80/zentao///");
    await wait(() => field("服务地址").value.endsWith("///"));
    assert(
      "规范化等价地址保留密码和HTTP许可",
      field("密码").readOnly && field("我允许通过").checked,
    );
    fill(field("服务地址"), "https://moved.example.test/zentao");
    await wait(() => !field("密码").readOnly);
    assert(
      "迁址清空密码掩码并要求重新登录",
      field("密码").required &&
        field("密码").value === "" &&
        field("密码").placeholder === "请输入密码" &&
        submit().textContent.includes("登录并保存") &&
        ![...doc.querySelectorAll("button")].some((button) =>
          button.textContent.includes("保留原密码"),
        ),
    );
    assert(
      "迁址提示保留同实例项目与执行关联",
      doc.body.textContent.includes("适用于当前禅道实例迁址"),
    );
    submit().click();
    assert("未输入新密码不能提交迁址", requests.length === 4);
    fill(field("密码"), "fixture-first-address-password");
    await wait(() => field("密码").value === "fixture-first-address-password");
    fill(field("服务地址"), "http://moved.example.test:8080/zentao");
    await wait(() => field("我允许通过") && field("密码").value === "");
    assert(
      "再次改址清空输入凭据且重新要求HTTP许可",
      !field("我允许通过").checked && submit().disabled,
    );
    fill(field("密码"), "fixture-moved-password");
    field("我允许通过").click();
    await wait(() => !submit().disabled);
    submit().click();
    await wait(() => requests.length === 5);
    assert(
      "迁址提交原连接ID和版本以及本次新密码",
      requests[4].args.connection.id === "fixture-connection" &&
        requests[4].args.connection.revision === 1 &&
        requests[4].args.connection.baseUrl === "http://moved.example.test:8080/zentao" &&
        requests[4].args.account === "fixture-user" &&
        requests[4].args.password === "fixture-moved-password" &&
        requests[4].args.token === null &&
        requests[4].args.rememberCredentials === true &&
        requests[4].args.allowInsecureHttp === true,
    );
    requests[4].reject("模拟新地址登录失败");
    await wait(() => doc.querySelector("[role=alert]") && !submit().disabled);
    assert(
      "迁址失败保留新地址及输入以便重试",
      field("服务地址").value === "http://moved.example.test:8080/zentao" &&
        field("密码").value === "fixture-moved-password" &&
        doc.querySelector("[role=alert]").textContent.includes("模拟新地址登录失败"),
    );
    submit().click();
    await wait(() => requests.length === 6);
    requests[5].resolve({ id: "fixture-connection", revision: 2 });
    await wait(() => !doc.querySelector("form"));

    win.renderDialog(true, true, true);
    await wait(() => field("密码")?.readOnly);
    mode("访问令牌").click();
    await wait(() => field("访问令牌"));
    fill(field("访问令牌"), "fixture-original-address-token");
    await wait(() => field("访问令牌").value === "fixture-original-address-token");
    fill(field("服务地址"), "https://token.example.test/zentao");
    await wait(() => field("访问令牌").required);
    assert(
      "令牌迁址清空旧输入并要求新地址令牌",
      field("访问令牌").value === "" &&
        field("访问令牌").placeholder === "请输入新地址的访问令牌" &&
        !field("记住登录凭据"),
    );
    submit().click();
    assert("缺少新令牌不能提交迁址", requests.length === 6);
    fill(field("访问令牌"), "fixture-moved-token");
    await wait(() => field("访问令牌").value === "fixture-moved-token");
    submit().click();
    await wait(() => requests.length === 7);
    assert(
      "令牌迁址提交新令牌并停用旧登录凭据",
      requests[6].args.connection.baseUrl === "https://token.example.test/zentao" &&
        requests[6].args.token === "fixture-moved-token" &&
        requests[6].args.account === null &&
        requests[6].args.password === null &&
        requests[6].args.rememberCredentials === false,
    );
    requests[6].reject("模拟令牌保存失败");
    await wait(() => !submit().disabled);
    fill(field("服务地址"), "http://example.test/zentao");
    await wait(() => !field("访问令牌").required);
    assert(
      "改回原地址可保留原令牌且清空其他地址的输入",
      field("访问令牌").value === "" &&
        field("访问令牌").placeholder === "留空保留已有令牌",
    );
    win.renderDialog(true, false, true, false);
    await wait(() => field("访问令牌")?.required && !submit().disabled);
    assert(
      "旧连接无本地凭据时明确要求重新输入",
      field("访问令牌").placeholder === "请输入访问令牌" &&
        doc.body.textContent.includes("此连接尚未保存本地凭据"),
    );
    submit().click();
    assert("缺少本地令牌时不能假装保留原令牌", requests.length === 7);
    assert("迁址流程没有浏览器异常", errors.length === 0);
    return results;
  } finally {
    frame.remove();
  }
})();
