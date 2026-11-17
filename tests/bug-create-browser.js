// node tests/bug-create-browser.js http://127.0.0.1:1421
// 浏览器仅调用模拟 IPC，不读取或写入实际工作空间及禅道。
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";

const origin = process.argv[2] || "http://127.0.0.1:1421";
const run = (...args) => execFileSync("npx", ["--yes", "agent-browser", "--session", "bug-create-test", ...args],
  { encoding: "utf8", timeout: 30_000 });
const evaluate = code => JSON.parse(JSON.parse(run("eval", `JSON.stringify(${code})`).trim()));
const dialog = "[role=combobox]";
const select = (label, value) => {
  run("click", `dialog ${dialog}[aria-label="${label}"]`);
  run("find", "role", "option", "click", "--name", value);
};
const click = label => run("find", "role", "button", "click", "--name", label, "--exact");
const calls = name => evaluate(`window.__bugCalls.filter(call => call.name === ${JSON.stringify(name)}).map(call => call.args)`);
const titleInput = 'dialog input[placeholder="一句话说明遇到的问题"]';
const fill = (selector, value) => run("fill", selector, value);

try {
  run("open", `${origin}/tests/bugs.html?localOnly`);
  click("新建 BUG");
  assert.equal(evaluate(`!!document.querySelector('dialog [aria-label="指派给"]')`), false);
  fill(titleInput, "离线创建 BUG");
  fill("dialog textarea", "回车时出现 <b>错误</b>");
  click("保存本地 BUG");
  run("wait", ".bugs-title");
  assert.match(evaluate("document.querySelector('.bugs-title').textContent"), /本地 BUG.*离线创建 BUG/s);
  const [local] = calls("local_bug_save");
  assert.equal(local.workspaceId, "bug-fixture");
  assert.equal(local.value.steps, "回车时出现 <b>错误</b>");
  for (const key of ["assignedTo", "projectId", "productId", "executionId", "mailto", "openedBuild"]) {
    assert.equal(Object.hasOwn(local.value, key), false, `本地保存不应提交 ${key}`);
  }
  assert.equal(calls("zentao_bug_create_options").length, 0);
  run("click", ".bugs-title");
  select("本地 BUG 状态", "已解决");
  click("保存本地 BUG");
  run("wait", ".bug-status-resolved");
  const [, update] = calls("local_bug_save");
  assert.equal(update.value.id, local.value.id);
  assert.equal(update.value.revision, 1);
  assert.equal(update.value.status, "resolved");

  run("open", `${origin}/tests/bugs.html`);
  click("新建 BUG");
  fill(titleInput, "提交到禅道");
  evaluate("window.__bugOptionsFail = true");
  select("禅道项目", "测试项目 #11");
  run("wait", "--text", "模拟：项目选项读取失败");
  evaluate("window.__bugOptionsFail = false");
  click("重新加载项目选项");
  run("wait", 'dialog [aria-label="所属产品"]');
  assert.match(evaluate(`document.querySelector('dialog [aria-label="所属产品"]').textContent`), /测试产品/);
  select("所属执行", "测试执行");
  select("指派给", "吴敏政（wuminzheng）");
  select("添加抄送人", "测试用户（tester）");
  evaluate("window.__bugCreateFail = true");
  click("提交到禅道");
  run("wait", "--text", "模拟：提交失败");
  assert.equal(evaluate(`document.querySelector(${JSON.stringify(titleInput)}).value`), "提交到禅道");
  evaluate("window.__bugCreateFail = false");
  click("提交到禅道");
  run("wait", "--fn", "!document.querySelector('dialog[open]')");
  const [failed, retried] = calls("zentao_bug_create");
  assert.deepEqual(failed, retried, "重试使用相同草稿编号和字段");
  assert.deepEqual({
    project: retried.value.projectId, product: retried.value.productId,
    execution: retried.value.executionId, assignee: retried.value.assignedTo,
    mailto: retried.value.mailto, openedBuild: retried.value.openedBuild,
  }, { project: "mock-project", product: "21", execution: "31", assignee: "wuminzheng", mailto: ["tester"], openedBuild: ["trunk"] });
  assert.equal(calls("local_bug_save").length, 0);
  assert.equal(evaluate("window.__fixtureBugs.some(bug => bug.remoteId === '299')"), false);

  click("新建 BUG");
  select("禅道项目", "测试项目 #11");
  run("wait", 'dialog [aria-label="指派给"]');
  select("禅道项目", "不选禅道项目（本地 BUG）");
  assert.equal(evaluate(`!!document.querySelector('dialog [aria-label="指派给"]')`), false);
  run("open", `${origin}/tests/bugs.html?resolvedByMe`);
  run("find", "role", "button", "click", "--name", "我解决的");
  assert.equal(evaluate("document.querySelectorAll('.bugs-title').length"), 1);
  run("click", ".bugs-title");
  run("wait", "--text", "我解决的 · 只读");
  const actions = evaluate("[...document.querySelectorAll('dialog button')].map(button => button.textContent.trim())");
  assert.ok(actions.includes("在禅道打开"));
  for (const action of ["编辑 BUG", "确认", "指派", "关闭", "激活", "解决"]) {
    assert.ok(!actions.includes(action), `只读记录不可${action}`);
  }
  console.log("通过：无连接新建及修改本地 BUG；切换创建方式；禅道选人抄送及保留草稿；查看解决后转派的禅道 BUG。");
} finally {
  run("close");
}
