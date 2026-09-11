// agent-browser eval --stdin < tests/mail-browser.js；所有邮箱和发信均为隔离模拟。
(async () => {
  const frame = document.createElement("iframe");
  frame.style.cssText =
    "position:fixed;inset:0;width:100vw;height:100vh;z-index:99999;border:0";
  document.body.append(frame);
  const win = frame.contentWindow,
    doc = frame.contentDocument;
  for (const n of document.querySelectorAll("style,link[rel=stylesheet]"))
    doc.head.append(n.cloneNode(true));
  doc.documentElement.dataset.theme = "light";
  doc.documentElement.style.colorScheme = "light";
  const shell = document.querySelector(".app-shell").cloneNode(true);
  const host = shell.querySelector("main");
  host.innerHTML = "";
  host.className = "mail-page";
  const notice = shell.querySelector(".preview-notice");
  if (notice)
    notice.textContent = "邮件验收 · 模拟邮箱与收发，不连接真实服务器";
  doc.body.append(shell);
  const requests = [],
    results = [],
    errors = [],
    notices = [];
  const accounts = [
    {
      id: "test-work",
      revision: 1,
      name: "工作邮箱（测试）",
      email: "work@example.test",
      senderName: "测试用户",
      username: "work@example.test",
      imapHost: "imap.example.test",
      imapPort: 993,
      imapSecurity: "tls",
      smtpHost: "smtp.example.test",
      smtpPort: 465,
      smtpSecurity: "tls",
      smtpUsername: "work@example.test",
      enabled: true,
      syncIntervalSeconds: 60,
      lastSync: Date.now(),
    },
    {
      id: "test-personal",
      revision: 1,
      name: "个人邮箱（测试）",
      email: "personal@example.test",
      senderName: "测试用户",
      username: "personal@example.test",
      imapHost: "imap.example.test",
      imapPort: 993,
      imapSecurity: "tls",
      smtpHost: "smtp.example.test",
      smtpPort: 465,
      smtpSecurity: "tls",
      smtpUsername: "personal@example.test",
      enabled: true,
      syncIntervalSeconds: 60,
    },
  ];
  const folders = [
    { path: "INBOX", name: "INBOX", kind: "inbox", unread: 2, total: 4 },
    { path: "Sent", name: "Sent", kind: "sent", unread: 0, total: 1 },
    { path: "Trash", name: "Trash", kind: "trash", unread: 0, total: 0 },
    { path: "Junk", name: "Junk", kind: "junk", unread: 0, total: 0 },
  ];
  const messages = [
    "本周项目安排与会议纪要",
    "设计评审材料已更新",
    "关于下周版本发布的准备工作",
    "月度工作总结",
  ].map((subject, i) => ({
    id: "m" + i,
    accountId: "test-work",
    folder: "INBOX",
    uid: i + 1,
    subject,
    from: [
      "产品团队 <team@example.test>",
      "设计协作 <design@example.test>",
      "研发团队 <dev@example.test>",
      "人事行政 <hr@example.test>",
    ][i],
    to: ["work@example.test"],
    cc: ["peer@example.test"],
    date: Date.now() - i * 86400000,
    preview: "你好，附件是本周的工作安排，请在方便时查看并确认。",
    seen: i > 1,
    flagged: i === 1,
    hasAttachments: i === 0,
  }));
  const drafts = [];
  win.isTauri = true;
  win.__TAURI_INTERNALS__ = {
    invoke: async (name, args = {}) => {
      requests.push({ name, args });
      if (name === "mail_accounts") return accounts;
      if (name === "mail_unread")
        return {
          total: messages.filter((m) => !m.seen).length,
          accounts: [
            {
              accountId: "test-work",
              unread: messages.filter((m) => !m.seen).length,
            },
          ],
        };
      if (name === "mail_folders") return folders;
      if (name === "mail_sync") return "模拟同步完成";
      if (name === "mail_messages") {
        const rows =
          args.folder === "INBOX"
            ? messages.filter((m) => m.subject.includes(args.search))
            : [];
        return { messages: rows, total: rows.length };
      }
      if (name === "mail_message") {
        const m = messages.find((m) => m.id === args.messageId);
        return {
          ...m,
          text: "你好，\n\n本周重点推进项目看板和邮箱功能。请确认以下安排：\n\n周一：需求与设计评审\n周三：开发进度同步\n周五：版本验收\n\n谢谢。",
          html: '<h2>本周工作安排</h2><p>这是正常正文。</p><script>parent.evil=true</script><img src="https://tracker.invalid/pixel"><form action="https://bad.invalid"><input></form><a href="javascript:alert(1)">恶意链接</a>',
          attachments: m.hasAttachments
            ? [
                {
                  id: "1",
                  name: "本周工作安排.pdf",
                  size: 20480,
                  contentType: "application/pdf",
                },
              ]
            : [],
        };
      }
      if (name === "mail_message_flag") {
        const m = messages.find((m) => m.id === args.messageId);
        if (args.seen !== null) m.seen = args.seen;
        if (args.flagged !== null) m.flagged = args.flagged;
        return;
      }
      if (name === "mail_drafts") return drafts;
      if (name === "mail_draft_save") {
        const d = { ...args.draft, updatedAt: Date.now() };
        drafts.push(d);
        return d;
      }
      if (name === "mail_send") return { message: "测试 SMTP 已接受邮件" };
      if (name === "mail_delivery_history")
        return [{ ...drafts[0], deliveryState: "uncertain" }];
      if (name === "mail_account_save")
        return { ...args.account, revision: 1, hasImapCredential: true };
      if (name === "tray_preferences")
        return { closeBehavior: "hide", trayAvailable: true };
      if (name === "tray_preferences_save")
        return { closeBehavior: args.closeBehavior, trayAvailable: true };
      throw Error("未预期 IPC " + name);
    },
  };
  win.addEventListener("error", (e) => errors.push(e.message));
  win.addEventListener("unhandledrejection", (e) =>
    errors.push(String(e.reason)),
  );
  win.testHost = host;
  win.testNotify = (m) => notices.push(m);
  const script = doc.createElement("script");
  script.type = "module";
  script.textContent = `import RefreshRuntime from '${location.origin}/@react-refresh';RefreshRuntime.injectIntoGlobalHook(window);window.$RefreshReg$=()=>{};window.$RefreshSig$=()=>type=>type;window.__vite_plugin_react_preamble_installed__=true;const {mountMailFixture}=await import('${location.origin}/tests/MailFixture.tsx');window.fixture=mountMailFixture(window.testHost,window.testNotify);window.fixture.draw('mail');`;
  doc.body.append(script);
  const wait = async (p) => {
    for (let i = 0; i < 160; i++) {
      if (p()) return;
      await new Promise((r) => setTimeout(r, 35));
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
    const prototype =
      input.tagName === "TEXTAREA"
        ? win.HTMLTextAreaElement.prototype
        : win.HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value").set.call(input, value);
    input.dispatchEvent(new win.Event("input", { bubbles: true }));
  };
  const field = (text) =>
    [...doc.querySelectorAll("dialog label")]
      .find((l) => l.textContent.trim().startsWith(text))
      ?.querySelector("input,textarea");
  try {
    await wait(() => doc.querySelectorAll(".mail-message-item").length === 4);
    assert(
      "多邮箱与文件夹显示",
      doc.querySelectorAll(".mail-tree-account").length === 2 &&
        doc.body.innerText.includes("已发送"),
    );
    doc.querySelector(".mail-message-item").click();
    await wait(() => doc.querySelector(".mail-envelope"));
    assert(
      "阅读邮件通过领域命令标为已读",
      requests.some(
        (r) =>
          r.name === "mail_message_flag" &&
          r.args.messageId === "m0" &&
          r.args.seen === true,
      ),
    );
    assert(
      "邮件正文与附件显示",
      doc.body.innerText.includes("周一：需求") &&
        doc.body.innerText.includes("本周工作安排.pdf"),
    );
    button("显示排版正文").click();
    await wait(() => doc.querySelector(".mail-html-content"));
    const html = doc.querySelector(".mail-html-content");
    assert(
      "HTML邮件隔离并拦截跟踪及脚本",
      html.getAttribute("sandbox") === "" &&
        !html.srcdoc.includes("<script>") &&
        !html.srcdoc.includes("tracker.invalid") &&
        !html.srcdoc.includes("<form"),
    );
    doc.querySelector('button[title="回复全部"]').click();
    await wait(() => field("收件人"));
    assert(
      "回复全部排除自己并包含抄送",
      field("收件人").value.includes("team@example.test") &&
        field("收件人").value.includes("peer@example.test") &&
        !field("收件人").value.includes("work@example.test"),
    );
    button("保存草稿").click();
    await wait(() => doc.body.innerText.includes("草稿已保存到本机"));
    const stored = requests.find((r) => r.name === "mail_draft_save");
    assert(
      "草稿使用数组地址契约且不触发发信",
      Array.isArray(stored.args.draft.to) &&
        !requests.some((r) => r.name === "mail_send"),
    );
    doc.querySelector('dialog button[title="关闭"]').click();
    await wait(() => !doc.querySelector("dialog"));
    doc.querySelector('button[title="回复"]').click();
    await wait(() => field("收件人"));
    button("发送").click();
    await wait(() => !doc.querySelector("dialog"));
    assert(
      "显式发送只提交一次并显示服务器结果",
      requests.filter((r) => r.name === "mail_send").length === 1 &&
        notices.includes("测试 SMTP 已接受邮件"),
    );
    (button("本机已发送") || button("已发送")).click();
    await wait(() => doc.body.innerText.includes("结果未确认"));
    assert("发送记录使用邮件列表而非弹窗", !doc.querySelector("dialog") && !!doc.querySelector(".mail-message-list .mail-message-item"));
    doc.querySelector(".mail-message-list .mail-message-item").click();
    await wait(() => doc.querySelector(".mail-reader .mail-text-content"));
    assert("发送记录正文在右侧阅读区显示", doc.querySelector(".mail-reader").textContent.includes("结果未确认") && !doc.querySelector("dialog"));
    assert("不确定投递不自动重发", requests.filter(r => r.name === "mail_send").length === 1);
    win.fixture.draw("settings");
    await wait(() => button("添加邮箱"));
    button("添加邮箱").click();
    await wait(() => field("显示名称"));
    fill(field("显示名称"), "新增测试邮箱");
    fill(field("邮箱地址"), "new@example.test");
    const serverInputs = [...doc.querySelectorAll("dialog input")].filter(
      (i) =>
        i.placeholder === "imap.example.com" ||
        i.placeholder === "smtp.example.com",
    );
    fill(serverInputs[0], "imap.example.test");
    fill(serverInputs[1], "smtp.example.test");
    fill(field("密码 / 授权码"), "fixture-only-secret");
    await new Promise((r) => setTimeout(r, 60));
    button("保存邮箱").click();
    await wait(() => requests.some((r) => r.name === "mail_account_save"));
    const saved = requests.find((r) => r.name === "mail_account_save");
    assert(
      "账号凭据与元数据分开提交",
      saved.args.imapSecret === "fixture-only-secret" &&
        !JSON.stringify(saved.args.account).includes("fixture-only-secret") &&
        saved.args.account.username === "new@example.test",
    );
    await wait(() => !doc.querySelector("dialog"));
    win.fixture.draw("general");
    await wait(() => doc.querySelector('button[aria-label="关闭主窗口时"]'));
    const closeSelect = doc.querySelector('button[aria-label="关闭主窗口时"]');
    await wait(() => !closeSelect.disabled);
    assert("默认关闭到托盘", closeSelect.textContent.includes("托盘"));
    closeSelect.click();
    await wait(() => doc.querySelector("[role=option]"));
    [...doc.querySelectorAll("[role=option]")]
      .find((n) => n.textContent.includes("关闭窗口并退出"))
      .click();
    await wait(() => requests.some((r) => r.name === "tray_preferences_save"));
    assert(
      "通用设置保存原生关闭行为",
      requests.find((r) => r.name === "tray_preferences_save").args
        .closeBehavior === "close",
    );
    win.fixture.draw("mail");
    await wait(() => doc.querySelectorAll(".mail-message-item").length === 4);
    doc.querySelector(".mail-message-item").click();
    await wait(() => doc.querySelector(".mail-envelope"));
    assert("无浏览器异常", errors.length === 0);
    window.mailFixture = {
      frame,
      doc,
      dispose: () => {
        win.fixture.dispose();
        frame.remove();
      },
    };
    return results;
  } finally {
    if (!window.keepMailFixture) {
      win.fixture?.dispose();
      frame.remove();
    }
  }
})();
