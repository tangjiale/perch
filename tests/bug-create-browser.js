// node tests/bug-create-browser.js http://127.0.0.1:1421
// 浏览器仅调用模拟 IPC，不读取或写入实际工作空间及禅道。
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";

const origin = process.argv[2] || "http://127.0.0.1:1421";
const run = (...args) => execFileSync("npx", ["--yes", "agent-browser", "--session", `bug-create-test-${process.pid}`, ...args],
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
const stepsEditor = 'dialog .tiptap[aria-label="BUG 复现步骤富文本"]';
const stepsHtml = () => evaluate(`document.querySelector(${JSON.stringify(stepsEditor)}).innerHTML`);
const emptyCaretVisible = () => evaluate(`getComputedStyle(document.querySelector('dialog .task-rich-body'), '::after').content !== 'none'`);
const stepsContent = () => evaluate(`(() => {
  const editor = document.querySelector(${JSON.stringify(stepsEditor)});
  return {
    text: editor.textContent,
    formattedText: [...editor.querySelectorAll('strong,em')].map(node => node.outerHTML),
    images: [...editor.querySelectorAll('img')].map(({ src, alt }) => ({ src, alt })),
  };
})()`);
const pasteHtml = html => evaluate(`(() => {
  const editor = document.querySelector(${JSON.stringify(stepsEditor)});
  editor.focus();
  const clipboardData = new DataTransfer();
  clipboardData.setData('text/html', ${JSON.stringify(html)});
  editor.dispatchEvent(new ClipboardEvent('paste', { clipboardData, bubbles: true, cancelable: true }));
  return true;
})()`);
const insertImage = (method, name, oversized = false) => evaluate(`(() => {
  const editor = document.querySelector(${JSON.stringify(stepsEditor)});
  editor.focus();
  editor.scrollIntoView({ block: 'center' });
  const bytes = ${oversized} ? new Uint8Array(5 * 1024 * 1024 + 1)
    : Uint8Array.from(atob(window.__bugImageData.split(',')[1]), char => char.charCodeAt(0));
  const data = new DataTransfer();
  data.items.add(new File([bytes], ${JSON.stringify(name)}, {type: 'image/png'}));
  if (${JSON.stringify(method)} === 'paste') {
    editor.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
  } else if (${JSON.stringify(method)} === 'drop') {
    const box = editor.getBoundingClientRect();
    editor.dispatchEvent(new DragEvent('drop', {
      dataTransfer: data, clientX: box.left + 18, clientY: box.top + 18, bubbles: true, cancelable: true,
    }));
  } else {
    const input = editor.closest('.task-rich-text').querySelector('input[type=file]');
    input.files = data.files;
    input.dispatchEvent(new Event('change', { bubbles: true }));
  }
  return true;
})()`);
const waitImage = name => run("wait", `${stepsEditor} img[alt="${name}"]`);
const assertDraftOnly = () => {
  for (const name of ["zentao_task_image_upload", "zentao_task_image_read", "zentao_bug_image_read"]) {
    assert.equal(calls(name).length, 0, `编辑 BUG 草稿时不应调用 ${name}`);
  }
};

try {
  run("open", `${origin}/tests/bugs.html?localOnly`);
  click("新建 BUG");
  assert.equal(evaluate(`!!document.querySelector('dialog [aria-label="指派给"]')`), false);
  fill(titleInput, "离线创建 BUG");
  run("wait", stepsEditor);
  evaluate(`(() => {
    const editor = document.querySelector(${JSON.stringify(stepsEditor)});
    // 模拟 WebView 没有把点击空白的默认焦点交给正文。
    editor.addEventListener('mousedown', event => event.preventDefault(), { capture: true, once: true });
    return true;
  })()`);
  run("scrollintoview", stepsEditor);
  run("click", stepsEditor);
  assert.equal(evaluate(`(() => {
    const editor = document.querySelector(${JSON.stringify(stepsEditor)});
    const selection = getSelection();
    return document.activeElement === editor && selection.isCollapsed && editor.contains(selection.anchorNode);
  })()`), true, "点空白处时应在正文建立插入光标");
  assert.equal(emptyCaretVisible(), true, "空白正文获焦时应显示首行光标");
  run("keyboard", "type", "字");
  assert.equal(stepsContent().text, "字");
  assert.equal(emptyCaretVisible(), false, "输入后应交回原生光标");
  run("press", "Backspace");
  assert.equal(stepsContent().text, "");
  assert.equal(emptyCaretVisible(), true, "删空正文后应重新显示首行光标");
  const hint = evaluate("document.querySelector('dialog .task-rich-hint').textContent");
  assert.match(hint, /步骤、实际结果和预期结果/);
  assert.match(hint, /单张不超过 5 MB，最多 8 张，合计不超过 12 MB/);
  pasteHtml(`<p>回车时出现 <strong>错误</strong><img src="${origin}/tests/bug-draft-image-must-not-load.png"><image src="${origin}/tests/bug-draft-image-must-not-load-alias.png"></p>`);
  run("wait", "--text", "已忽略外部图片");
  assert.match(stepsHtml(), /<strong>错误<\/strong>/);
  assert.doesNotMatch(stepsHtml(), /bug-draft-image-must-not-load/);
  assert.equal(evaluate("performance.getEntriesByType('resource').some(entry => entry.name.includes('bug-draft-image-must-not-load'))"), false);
  insertImage("paste", "超限.png", true);
  run("wait", "--text", "请选择不超过 5 MB");
  insertImage("paste", "本地截图.png");
  waitImage("本地截图.png");
  assert.equal(evaluate("!!document.querySelector('dialog [aria-label=插入图片]')"), true);
  insertImage("file", "待删除.png");
  waitImage("待删除.png");
  assert.equal(stepsContent().images.length, 2, "连续插入应保留此前选中的图片");
  const removableImage = `${stepsEditor} img[alt="待删除.png"]`;
  run("scrollintoview", removableImage);
  run("click", removableImage);
  run("wait", "dialog.image-viewer[open] .image-viewer-image");
  run("click", 'dialog.image-viewer button[aria-label="关闭"]');
  run("scrollintoview", 'dialog .task-rich-toolbar [aria-label="删除选中的图片"]');
  click("删除选中的图片");
  assert.doesNotMatch(stepsHtml(), /待删除\.png/);
  assertDraftOnly();
  click("保存本地 BUG");
  run("wait", ".bugs-title");
  assert.match(evaluate("document.querySelector('.bugs-title').textContent"), /本地 BUG.*离线创建 BUG/s);
  const [local] = calls("local_bug_save");
  assert.equal(local.workspaceId, "bug-fixture");
  assert.equal(local.value.stepsFormat, "html");
  assert.match(local.value.steps, /<strong>错误<\/strong>/);
  assert.match(local.value.steps, /<img src="data:image\/png;base64,[^"]+" alt="本地截图\.png"/);
  assert.doesNotMatch(local.value.steps, /待删除\.png|bug-draft-image-must-not-load/);
  for (const key of ["assignedTo", "projectId", "productId", "executionId", "mailto", "openedBuild"]) {
    assert.equal(Object.hasOwn(local.value, key), false, `本地保存不应提交 ${key}`);
  }
  assert.equal(calls("zentao_bug_create_options").length, 0);
  run("click", ".bugs-title");
  waitImage("本地截图.png");
  select("本地 BUG 状态", "已解决");
  click("保存本地 BUG");
  run("wait", ".bug-status-resolved");
  const [, update] = calls("local_bug_save");
  assert.equal(update.value.id, local.value.id);
  assert.equal(update.value.revision, 1);
  assert.equal(update.value.status, "resolved");
  assert.equal(update.value.steps, local.value.steps);
  assert.equal(Object.hasOwn(update.value, "stepsFormat"), false, "未修改的复现步骤须原样提交");

  run("open", `${origin}/tests/bugs.html`);
  click("新建 BUG");
  fill(titleInput, "提交到禅道");
  run("wait", stepsEditor);
  pasteHtml(evaluate("'<p>步骤一：<em>重新登录</em></p><img src=\"' + window.__bugImageData + '\" alt=\"随项目保留.png\">'"));
  waitImage("随项目保留.png");
  const beforeProject = stepsContent();
  evaluate("window.__bugOptionsFail = true");
  select("禅道项目", "测试项目 #11");
  run("wait", "--text", "模拟：项目选项读取失败");
  evaluate("window.__bugOptionsFail = false");
  click("重新加载项目选项");
  run("wait", 'dialog [aria-label="所属产品"]');
  assert.match(evaluate(`document.querySelector('dialog [aria-label="所属产品"]').textContent`), /测试产品/);
  assert.deepEqual(stepsContent(), beforeProject);
  select("禅道项目", "不选禅道项目（本地 BUG）");
  assert.equal(evaluate(`!!document.querySelector('dialog [aria-label="指派给"]')`), false);
  assert.deepEqual(stepsContent(), beforeProject);
  select("禅道项目", "测试项目 #11");
  run("wait", 'dialog [aria-label="所属产品"]');
  select("所属执行", "测试执行");
  select("指派给", "吴敏政（wuminzheng）");
  select("添加抄送人", "测试用户（tester）");
  evaluate("window.__bugHoldImageRead = true");
  insertImage("drop", "禅道草稿截图.png");
  run("wait", "--fn", "typeof window.__bugReleaseImageRead === 'function'");
  assert.deepEqual(evaluate(`[
    document.querySelector('dialog [aria-label="关闭"]').disabled,
    document.querySelector('dialog [aria-label="禅道项目"]').disabled,
    ...[...document.querySelectorAll('dialog footer button')].map(button => button.disabled),
  ]`), [true, true, true, true], "读取图片时禁止关闭、切换项目和提交");
  evaluate("document.querySelector('dialog form').dispatchEvent(new Event('submit', {bubbles: true, cancelable: true}))");
  assert.equal(calls("zentao_bug_create").length, 0);
  evaluate("(window.__bugHoldImageRead = false, window.__bugReleaseImageRead(), true)");
  waitImage("禅道草稿截图.png");
  const beforeSubmit = stepsContent();
  assertDraftOnly();
  evaluate("window.__bugCreateFail = true");
  click("提交到禅道");
  run("wait", "--text", "模拟：提交失败");
  assert.equal(evaluate(`document.querySelector(${JSON.stringify(titleInput)}).value`), "提交到禅道");
  assert.deepEqual(stepsContent(), beforeSubmit, "提交失败保留富文本与图片");
  evaluate("window.__bugCreateFail = false");
  click("提交到禅道");
  run("wait", "--fn", "!document.querySelector('dialog[open]')");
  const [failed, retried] = calls("zentao_bug_create");
  assert.deepEqual(failed, retried, "重试使用相同草稿编号和字段");
  assert.equal(retried.value.stepsFormat, "html");
  assert.match(retried.value.steps, /<em>重新登录<\/em>/);
  assert.match(retried.value.steps, /<img src="data:image\/png;base64,[^"]+" alt="禅道草稿截图\.png"/);
  assert.deepEqual({
    project: retried.value.projectId, product: retried.value.productId,
    execution: retried.value.executionId, assignee: retried.value.assignedTo,
    mailto: retried.value.mailto, openedBuild: retried.value.openedBuild,
  }, { project: "mock-project", product: "21", execution: "31", assignee: "wuminzheng", mailto: ["tester"], openedBuild: ["trunk"] });
  assert.equal(calls("local_bug_save").length, 0);
  assertDraftOnly();
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
  console.log("通过：本地和禅道 BUG 富文本及图片，过滤外部图片，读图锁定，切换项目和失败重试保留草稿，已有只读 BUG 权限。");
} finally {
  run("close");
}
