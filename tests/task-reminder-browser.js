// 仅在隔离 fixture 中执行，不访问真实通知权限或桌面 IPC。
(async () => {
  if (location.pathname !== "/tests/execution-create.html")
    throw Error("需要隔离测试页面");
  const results = [];
  const check = (name, passed) => {
    if (!passed) throw Error(name);
    results.push(name);
  };
  const wait = async (predicate) => {
    for (let i = 0; i < 100; i++) {
      if (predicate()) return;
      await new Promise((resolve) => setTimeout(resolve, 30));
    }
    throw Error("等待超时");
  };
  const reminder = () => document.querySelector('[aria-label="到期提醒"]');
  const submit = () => document.querySelector("form").dispatchEvent(
    new Event("submit", { bubbles: true, cancelable: true }),
  );
  const draw = async (overrides = {}) => {
    window.drawTask(overrides);
    await new Promise((resolve) => setTimeout(resolve, 60));
    await wait(() => reminder());
  };
  window.calls = [];
  window.reminderPermission = true;
  await draw();
  check("旧任务默认关闭提醒", !reminder().checked);
  await draw({ schedule: undefined });
  check("无结束时间不能开启提醒", reminder().disabled && !reminder().checked);
  await draw();
  reminder().click();
  await wait(() => document.querySelector('[aria-label="提醒时间"]'));
  document.querySelector('[aria-label="提醒时间"]').click();
  await wait(() => document.querySelector('[role="option"]'));
  [...document.querySelectorAll('[role="option"]')]
    .find((node) => node.textContent.includes("提前 1 小时")).click();
  await wait(() => document.querySelector('[aria-label="提醒时间"]').textContent.includes("提前 1 小时"));
  submit();
  await wait(() => window.calls.length === 1);
  check("提醒设置随任务保存", window.calls[0].value.reminder.enabled &&
    window.calls[0].value.reminder.advanceMinutes === 60);
  window.calls[0].resolve(window.calls[0].value);
  await wait(() => !document.querySelector("form"));
  await draw({ reminder: { enabled: true, advanceMinutes: 1440 } });
  check("提醒设置回显", reminder().checked &&
    document.querySelector('[aria-label="提醒时间"]').textContent.includes("提前 1 天"));
  const end = [...document.querySelectorAll("label")]
    .find((node) => node.textContent.trim().startsWith("结束时间")).querySelector("input");
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(end, "");
  end.dispatchEvent(new Event("input", { bubbles: true }));
  await new Promise((resolve) => setTimeout(resolve, 60));
  submit();
  await wait(() => document.querySelector('[role="alert"]'));
  check("已启用提醒清空结束时间不能保存", window.calls.length === 1 &&
    document.querySelector('[role="alert"]').textContent.includes("结束日期时间"));
  await draw({ reminder: { enabled: true, advanceMinutes: 15 } });
  window.reminderPermission = false;
  submit();
  await wait(() => document.querySelector('[role="alert"]'));
  check("拒绝通知权限保留表单且不保存", window.calls.length === 1 &&
    reminder().checked && document.querySelector('[role="alert"]').textContent.includes("系统设置"));
  window.reminderPermission = true;
  await draw({ schedule: { kind: "all_day", start: "2026-10-12", end: "2026-10-13", timezone: "Asia/Shanghai" } });
  check("全天到期语义明确", document.querySelector("form").textContent.includes("结束日期次日零点到期"));
  return results;
})();
