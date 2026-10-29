// 在 /tests/meetings.html 中通过 agent-browser eval --stdin 执行；仅内存 IPC。
(async () => {
  const checks = [];
  const tick = () => new Promise((resolve) => setTimeout(resolve, 80));
  const button = (text) => [...document.querySelectorAll("button")].find((element) => element.textContent.trim() === text);
  const assert = (condition, label) => { if (!condition) throw new Error(label); checks.push(label); };
  const input = document.querySelector('[aria-label="会议名称"]');
  assert(!!input, "已读取模拟会议");
  const fill = (element, value) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(element, value);
    element.dispatchEvent(new Event("input", { bubbles: true }));
  };
  window.__saveFail = true;
  fill(input, "草稿保留验收");
  await tick();
  button("保存修改").click();
  await tick();
  assert(input.value === "草稿保留验收" && document.body.textContent.includes("模拟保存失败"), "保存失败保留草稿并显示原因");
  assert(button("新建").disabled && button("继续录音").disabled, "未保存修改阻止新建和录音冲突");
  window.__saveFail = false;
  button("保存修改").click();
  await tick();
  assert(window.__meetings[0].title === "草稿保留验收" && button("保存").disabled, "显式重试保存成功");
  button("会议纪要").click();
  await tick();
  assert(document.body.textContent.includes("验证不同说话人识别") && document.body.textContent.includes("下周三"), "纪要包含结论和行动项负责人期限");
  window.__generateFail = true;
  button("生成纪要与脑图").click();
  await tick();
  assert(document.body.textContent.includes("模拟模型服务不可用") && document.body.textContent.includes("团队确认会议记录"), "生成失败保留原纪要");
  window.__generateFail = false;
  button("会议脑图").click();
  await tick();
  assert(document.querySelector('svg[aria-label="会议脑图"] path') && document.querySelectorAll('svg[aria-label="会议脑图"] text').length === 7, "脑图渲染完整节点与关系线");
  const search = document.querySelector('[aria-label="搜索会议"]');
  fill(search, "不存在的会议");
  await tick();
  assert(document.body.textContent.includes("没有匹配的会议"), "搜索空结果有明确反馈");
  fill(search, "");
  await tick();
  assert(document.documentElement.scrollWidth <= innerWidth, "页面无横向溢出");
  assert(window.__calls.every((call) => call.args.workspaceId === "fixture"), "领域 IPC 均绑定当前工作空间");
  return { passed: checks.length, checks, viewport: [innerWidth, innerHeight] };
})()
