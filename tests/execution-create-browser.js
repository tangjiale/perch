// 仅在隔离 fixture 中执行；不访问真实桌面 IPC 或禅道。
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
  const field = (label) =>
    [...document.querySelectorAll("label")]
      .find((node) => node.textContent.trim().startsWith(label))
      ?.querySelector("input");
  const fill = (input, value) => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    ).set.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const submit = () =>
    document
      .querySelector("form")
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  const chooseProject = async (name) => {
    document.querySelector('[aria-label="所属项目"]').click();
    await wait(() => document.querySelector('[role="option"]'));
    [...document.querySelectorAll('[role="option"]')]
      .find((node) => node.textContent.includes(name))
      .click();
    await wait(() =>
      document
        .querySelector('[aria-label="所属项目"]')
        .textContent.includes(name),
    );
  };
  await wait(() => field("任务名称"));
  await chooseProject("禅道项目");
  check(
    "禅道项目自动转换日期且保留原日期",
    field("开始日期").value === "2026-09-08" &&
      field("结束日期").value === "2026-09-10",
  );
  check(
    "新执行固定待做及全天",
    document.querySelector('[aria-label="任务状态"]').disabled &&
      document
        .querySelector('[aria-label="任务状态"]')
        .textContent.includes("待做") &&
      field("全天安排").checked &&
      field("全天安排").disabled,
  );
  fill(field("结束日期"), "");
  await new Promise((resolve) => setTimeout(resolve, 40));
  submit();
  await wait(() => document.querySelector('[role="alert"]'));
  check("缺少计划结束日期不能提交", window.calls.length === 0);
  window.drawTask({ projectId: "remote" }, true);
  await wait(
    () =>
      field("结束日期")?.value === "2026-09-10" &&
      !document.querySelector('[role="alert"]'),
  );
  submit();
  submit();
  await wait(() => window.calls.length === 1);
  check(
    "连续提交只发送一次且期间不可退出",
    document.querySelector('button[title="关闭"]').disabled &&
      window.calls.length === 1,
  );
  check(
    "全天结束日期为排他次日且不伪造远端关联",
    window.calls[0].value.schedule.end === "2026-09-11" &&
      window.calls[0].value.source === "local" &&
      !window.calls[0].value.remoteId,
  );
  window.calls[0].resolve({
    ...window.calls[0].value,
    revision: 1,
    source: "zentao",
    remoteId: "42",
  });
  await wait(() =>
    document
      .querySelector('[role="alert"]')
      ?.textContent.includes("保存已成功"),
  );
  submit();
  check(
    "成功但刷新失败不会重复创建",
    window.calls.length === 1 &&
      [...document.querySelectorAll("button")].some(
        (button) => button.textContent === "已保存" && button.disabled,
      ),
  );
  window.drawTask({ projectId: "local" });
  await wait(
    () => field("开始时间") && !document.querySelector('[role="alert"]'),
  );
  submit();
  await wait(() => window.calls.length === 2);
  check(
    "本地项目保留时间与原状态",
    window.calls[1].value.status === "doing" &&
      window.calls[1].value.schedule.kind === "timed" &&
      window.calls[1].value.projectId === "local",
  );
  window.calls[1].resolve({ ...window.calls[1].value, revision: 1 });
  await wait(() => !document.querySelector("dialog"));
  window.drawTask({ revision: 2, projectId: "remote" });
  await wait(() => field("开始时间"));
  check(
    "编辑已有本地任务不会触发新执行模式",
    !document.querySelector('[aria-label="任务状态"]').disabled &&
      !document.body.textContent.includes("默认迭代"),
  );
  return results;
})();
