// Vite 页面运行：agent-browser eval --stdin < tests/provider-models-browser.js。
// 仅隔离 iframe 模拟 IPC，不接触真实配置或密钥。设置 window.keepModelFixture=true 可保留截图。
(async () => {
  const frame = document.createElement("iframe");
  frame.style.cssText =
    "position:fixed;inset:0;width:100vw;height:100vh;border:0;z-index:99999";
  document.body.append(frame);
  const win = frame.contentWindow,
    doc = frame.contentDocument;
  for (const node of document.querySelectorAll('style,link[rel="stylesheet"]'))
    doc.head.append(node.cloneNode(true));
  doc.documentElement.dataset.theme = "light";
  doc.documentElement.style.colorScheme = "light";
  const requests = [],
    errors = [],
    results = [];
  win.isTauri = true;
  win.refreshFails = false;
  win.__TAURI_INTERNALS__ = {
    invoke: (name, args) =>
      new Promise((resolve, reject) =>
        requests.push({ name, args, resolve, reject }),
      ),
  };
  win.addEventListener("error", (e) => errors.push(e.message));
  win.addEventListener("unhandledrejection", (e) =>
    errors.push(String(e.reason)),
  );
  const script = doc.createElement("script");
  script.type = "module";
  script.textContent = `
    import RefreshRuntime from '${location.origin}/@react-refresh';
    RefreshRuntime.injectIntoGlobalHook(window);
    window.$RefreshReg$ = () => {}; window.$RefreshSig$ = () => type => type;
    window.__vite_plugin_react_preamble_installed__ = true;
    const { mountSettings } = await import('${location.origin}/tests/ProviderModelsFixture.tsx');
    const host = document.createElement('main'); document.body.append(host);
    window.dispose = mountSettings(host, async () => { if(window.refreshFails) throw Error('模拟刷新失败'); });
    window.ready = true;
  `;
  doc.body.append(script);
  const wait = async (predicate) => {
    for (let i = 0; i < 120; i++) {
      if (predicate()) return;
      await new Promise((r) => setTimeout(r, 30));
    }
    throw Error(
      "等待超时 " + JSON.stringify({ errors, text: doc.body.innerText }),
    );
  };
  const assert = (name, value) => {
    if (!value) throw Error(name);
    results.push({ name, passed: true });
  };
  const button = (text) =>
    [...doc.querySelectorAll("button")].find(
      (b) => b.textContent.trim() === text,
    );
  const fill = (input, value) => {
    Object.getOwnPropertyDescriptor(
      win.HTMLInputElement.prototype,
      "value",
    ).set.call(input, value);
    input.dispatchEvent(new win.Event("input", { bubbles: true }));
  };
  const catalog = {
    providerId: "fixture-provider",
    providerRevision: 2,
    workspaceId: "fixture",
    generation: "fixture-generation",
    models: [
      { id: "existing-chat", name: "已添加模型" },
      { id: "chat-new", name: "通用对话模型" },
      { id: "embed-new", name: "知识库向量模型" },
      { id: "vision-new", name: "视觉模型" },
      {
        id: "long-model-" + "context-".repeat(25),
        name: "用于验证超长名称换行的模型".repeat(8),
      },
    ],
  };
  try {
    await wait(() => win.ready && button("配置"));
    button("配置").click();
    await wait(() => button("保存并拉取模型"));
    fill(doc.querySelector('input[type="url"]'), "http://localhost:9000/v1");
    await new Promise((r) => setTimeout(r, 50));
    button("保存并拉取模型").click();
    button("保存并拉取模型")?.click();
    await wait(() => requests.length === 1);
    assert(
      "保存最新配置且阻止重复提交",
      requests[0].name === "save_provider" &&
        requests[0].args.value.baseUrl === "http://localhost:9000/v1",
    );
    requests[0].resolve({ ...requests[0].args.value, revision: 2 });
    await wait(() => requests.length === 2);
    assert(
      "保存后拉取且免密服务不写密钥",
      requests[1].name === "provider_models" &&
        requests[1].args.providerId === "fixture-provider",
    );
    requests[1].reject("模拟目录服务暂不可用");
    await wait(() => doc.querySelector("[role=alert]"));
    assert(
      "失败展示重试入口",
      !button("重新拉取").disabled &&
        doc.body.innerText.includes("模拟目录服务暂不可用"),
    );
    button("重新拉取").click();
    await wait(() => requests.length === 3);
    requests[2].resolve(catalog);
    await wait(() => doc.querySelectorAll(".model-catalog-row").length === 5);
    const existing = doc.querySelector(
      'input[aria-label="选择 existing-chat"]',
    );
    assert(
      "已有模型勾选并禁用",
      existing.checked &&
        existing.disabled &&
        doc.body.innerText.includes("已添加"),
    );
    const search = doc.querySelector('input[aria-label="搜索模型"]');
    fill(search, "embed");
    await wait(() => doc.querySelectorAll(".model-catalog-row").length === 1);
    button("全选当前结果").click();
    await wait(() => doc.body.innerText.includes("已选 1 个"));
    assert(
      "搜索后全选仅选择筛选结果",
      doc.querySelector('input[aria-label="选择 embed-new"]').checked,
    );
    assert(
      "无需展开菜单即可看到文本视觉向量",
      ["文本", "视觉", "向量"].every((label) =>
        [
          ...doc.querySelectorAll(
            'fieldset[aria-label="embed-new 模型类型"] span',
          ),
        ].some(
          (option) =>
            option.textContent === label &&
            option.getBoundingClientRect().height > 0,
        ),
      ),
    );
    doc
      .querySelector(
        'fieldset[aria-label="embed-new 模型类型"] input[value="embedding"]',
      )
      .click();
    await wait(
      () =>
        doc.querySelector(
          'fieldset[aria-label="embed-new 模型类型"] input[value="embedding"]',
        ).checked,
    );
    fill(search, "");
    await wait(() => doc.querySelectorAll(".model-catalog-row").length === 5);
    doc.querySelector('input[aria-label="选择 chat-new"]').click();
    await wait(() => doc.body.innerText.includes("已选 2 个"));
    doc.querySelector('input[aria-label="选择 vision-new"]').click();
    await wait(() => doc.body.innerText.includes("已选 3 个"));
    doc
      .querySelector(
        'fieldset[aria-label="vision-new 模型类型"] input[value="vision"]',
      )
      .click();
    await wait(
      () =>
        doc.querySelector(
          'fieldset[aria-label="vision-new 模型类型"] input[value="vision"]',
        ).checked,
    );
    win.refreshFails = true;
    button("添加所选模型（3）").click();
    await wait(() => requests.length === 4);
    assert(
      "批量提交保留模型类型与版本保护",
      requests[3].name === "provider_models_import" &&
        requests[3].args.models.length === 3 &&
        requests[3].args.models.find((s) => s.id === "chat-new").capability ===
          "chat" &&
        requests[3].args.models.find((s) => s.id === "vision-new")
          .capability === "vision" &&
        requests[3].args.models.find((s) => s.id === "embed-new").capability ===
          "embedding" &&
        requests[3].args.catalog.providerRevision === 2,
    );
    const cancel = new win.Event("cancel", { cancelable: true });
    doc.querySelector("dialog").dispatchEvent(cancel);
    assert(
      "写入中阻止关闭和 Escape",
      cancel.defaultPrevented &&
        doc.querySelector('button[title="关闭"]').disabled,
    );
    requests[3].resolve({ added: 3, skipped: 0 });
    await wait(() => button("重试刷新"));
    assert(
      "刷新失败明确提示已保存",
      doc.body.innerText.includes("模型已添加，列表刷新失败"),
    );
    win.refreshFails = false;
    button("重试刷新").click();
    await wait(() => !doc.querySelector("dialog"));
    assert("重试刷新不重复写入", requests.length === 4);
    // 重开以验证空目录，并保留代表性列表供深浅色截图。
    button("配置").click();
    await wait(() => button("保存并拉取模型"));
    button("保存并拉取模型").click();
    await wait(() => requests.length === 5);
    requests[4].resolve({ ...requests[4].args.value, revision: 3 });
    await wait(() => requests.length === 6);
    requests[5].resolve({ ...catalog, providerRevision: 3, models: [] });
    await wait(() => doc.body.innerText.includes("供应商返回了空目录"));
    assert("空目录不能提交", button("添加所选模型").disabled);
    button("重新拉取").click();
    await wait(() => requests.length === 7);
    requests[6].resolve(catalog);
    await wait(() => doc.querySelectorAll(".model-catalog-row").length === 5);
    button("全选当前结果").click();
    await wait(() => doc.body.innerText.includes("已选 4 个"));
    button("清空选择").click();
    await wait(() => doc.body.innerText.includes("已选 0 个"));
    assert(
      "清空只清除本次选择并保留已有标记",
      doc.querySelector('input[aria-label="选择 existing-chat"]').checked &&
        doc.querySelector('input[aria-label="选择 existing-chat"]').disabled,
    );
    button("返回配置").click();
    await wait(() => button("保存并拉取模型"));
    doc.querySelector('dialog header button[title="关闭"]').click();
    await wait(() => !doc.querySelector("dialog"));
    const statusSwitch = doc.querySelector('[role="switch"]');
    assert("模型列表显示禁用滑块及删除按钮", statusSwitch.getAttribute("aria-checked") === "false" && !!doc.querySelector('[aria-label="删除模型 已添加模型"]'));
    statusSwitch.click();
    statusSwitch.click();
    await wait(() => requests.length === 8);
    assert("切换保存正确状态和修订且防重复提交", requests[7].name === "save_model" && requests[7].args.value.enabled === true && requests[7].args.value.revision === 1 && statusSwitch.disabled);
    requests[7].reject("模拟保存失败");
    await wait(() => !statusSwitch.disabled);
    assert("保存失败保留原禁用状态", statusSwitch.getAttribute("aria-checked") === "false");
    doc.querySelector('[aria-label="删除模型 已添加模型"]').click();
    await wait(() => button("确认删除"));
    button("取消").click();
    await wait(() => !doc.querySelector("dialog"));
    assert("取消删除不发送请求", requests.length === 8);
    doc.querySelector('[aria-label="删除模型 已添加模型"]').click();
    await wait(() => button("确认删除"));
    button("确认删除").click();
    button("确认删除").click();
    await wait(() => requests.length === 9);
    assert("确认删除携带修订并防重复", requests[8].name === "delete_model" && requests[8].args.id === "existing" && requests[8].args.revision === 1);
    requests[8].reject("该记录仍被引用，请先解除关联或停用");
    await wait(() => doc.querySelector('[role="alert"]'));
    assert("引用中的模型删除失败提示并保留弹窗", doc.body.innerText.includes("请先解除关联或停用"));
    button("确认删除").click();
    await wait(() => requests.length === 10);
    requests[9].resolve();
    await wait(() => !doc.querySelector("dialog"));
    assert("删除成功后关闭确认窗口", true);
    assert("无浏览器异常", errors.length === 0);
    window.modelFixture = {
      frame,
      doc,
      dispose: () => {
        win.dispose();
        frame.remove();
      },
    };
    return results;
  } finally {
    if (!window.keepModelFixture) {
      win.dispose?.();
      frame.remove();
    }
  }
})();
