(async () => {
  const { native } = await import("/src/lib/api.ts");
  if (native) throw Error("仅隔离浏览器预览可运行");
  const { mountAssignment } = await import("/tests/TaskAssignmentFixture.tsx");
  const host = document.createElement("div");
  document.body.append(host);
  const fixture = mountAssignment(host);
  const waitFor = async check => {
    const deadline = performance.now() + 5000;
    while (!check()) {
      if (performance.now() > deadline) throw Error("等待指派信息状态超时");
      await new Promise(resolve => setTimeout(resolve, 40));
    }
  };
  const text = () => document.querySelector("dialog")?.textContent || "";
  const details = { assignedBy: "张经理", assignedAt: "2026-10-08 09:30:00", createdBy: "李同事" };
  try {
    await waitFor(() => fixture.requests.length === 1);
    if (!text().includes("正在读取指派信息")) throw Error("应显示独立加载状态");
    fixture.requests[0].resolve(details);
    await waitFor(() => text().includes("张经理"));
    if (!text().includes("指派时间") || !text().includes(details.assignedAt) || !text().includes("李同事"))
      throw Error("应显示操作者、时间与独立创建人");
    const titleInput = document.querySelector("dialog input");
    if (titleInput.disabled) throw Error("只读信息加载不得禁用任务编辑");
    fixture.show("unknown");
    await waitFor(() => fixture.requests.length === 2);
    fixture.requests[1].resolve({ assignedBy: null, assignedAt: null, createdBy: "只有创建人" });
    await waitFor(() => text().includes("未找到指派记录"));
    if (document.querySelector(".task-assignment-details dd").textContent !== "未找到指派记录")
      throw Error("不能用创建人冒充指派人");
    fixture.show("failure");
    await waitFor(() => fixture.requests.length === 3);
    fixture.requests[2].reject(Error("连接已停用"));
    await waitFor(() => text().includes("连接已停用"));
    [...document.querySelectorAll("dialog button")].find(button => button.textContent === "重试").click();
    await waitFor(() => fixture.requests.length === 4);
    fixture.requests[3].resolve(details);
    await waitFor(() => text().includes("张经理"));
    fixture.show("stale");
    await waitFor(() => fixture.requests.length === 5);
    fixture.show("current");
    await waitFor(() => fixture.requests.length === 6);
    fixture.requests[5].resolve(details);
    await waitFor(() => text().includes("张经理"));
    fixture.requests[4].resolve({ ...details, assignedBy: "迟到的旧任务指派人" });
    await new Promise(resolve => setTimeout(resolve, 80));
    if (text().includes("迟到的旧任务指派人")) throw Error("迟到结果不能覆盖当前任务");
    fixture.show("local", "local");
    await waitFor(() => !document.querySelector(".task-assignment-details"));
    if (fixture.requests.length !== 6) throw Error("本地任务不得查询禅道");
    fixture.show("visual");
    await waitFor(() => fixture.requests.length === 7);
    fixture.requests[6].resolve({ ...details, assignedBy: "张经理（飞英专家项目负责人）" });
    await waitFor(() => text().includes("张经理（飞英专家项目负责人）"));
    window.assignmentFixture = fixture;
    return { loading: true, actorAndTime: true, unknown: true, retry: true, staleResultIgnored: true, localNoQuery: true };
  } catch (error) {
    fixture.dispose(); host.remove(); throw error;
  }
})();
