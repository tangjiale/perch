// node tests/bug-history-browser.js http://127.0.0.1:1421
// 只访问隔离 fixture 的模拟 IPC，不读取真实禅道或凭据。
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";

const origin = process.argv[2] || "http://127.0.0.1:1421";
assert.ok(["127.0.0.1", "localhost"].includes(new URL(origin).hostname));
const run = (...args) => execFileSync("npx", ["--yes", "agent-browser", "--session", "bug-history-test", ...args], { encoding: "utf8", timeout: 60000 });
const ev = code => JSON.parse(JSON.parse(run("eval", `JSON.stringify(${code})`).trim()));
const wait = () => run("wait", "250");
const text = () => ev("document.querySelector('.bug-history').textContent");
const clickText = label => ev(`(() => { [...document.querySelectorAll('dialog button')].find(button => button.textContent.trim() === ${JSON.stringify(label)}).click(); return true; })()`);
const open = mode => {
  run("open", `${origin}/tests/bugs.html`); wait();
  ev(`window.__bugHistoryMode = ${JSON.stringify(mode || "")}`);
  run("click", ".bugs-title"); wait();
};
const close = () => { run("click", 'dialog [title="关闭"]'); wait(); };

open();
assert.match(text(), /历史记录/);
for (const content of ["指派人", "指派时间", "创建人", "吴敏政", "2026-09-08", "测试用户", "仅支持 dify", "后续扩展其他知识库"]) assert.ok(text().includes(content), `历史应展示 ${content}`);
assert.deepEqual(ev("window.__bugHistoryCalls.map(call => call.args)"), [{ bugId: "bug-0" }]);
assert.equal(ev("document.querySelectorAll('.bug-history-entry').length"), 3);
assert.deepEqual(ev("[...document.querySelectorAll('.bug-history-entry')].map(row => row.textContent.includes('创建问题') ? 'create' : row.textContent.includes('知识库接入') ? 'assign' : 'edit')"), ["create", "assign", "edit"], "历史按业务时间正序展示");
assert.equal(ev("!!document.querySelector('.bug-history strong, .bug-history b')"), true, "安全文字格式保留");
assert.equal(ev("!!document.querySelector('.bug-history img, .bug-history script, .bug-history svg, .bug-history a, .bug-history [onerror], .bug-history [onload]')"), false, "图片外链及危险节点不进入 DOM");
assert.equal(ev("!!window.__unsafeHistory"), false);
assert.deepEqual(ev("performance.getEntriesByType('resource').filter(item => item.name.includes('history-tracking.invalid')).map(item => item.name)"), [], "历史备注不加载外部图片");
ev("(() => { document.querySelectorAll('.bug-history details').forEach(details => { details.open = true; }); return true; })()");
assert.match(text(), /原始备注/);
assert.match(text(), /新的知识库标题/);
assert.equal(ev("window.__bugCalls.filter(call => ['zentao_bug_save', 'zentao_bug_transition'].includes(call.name)).length"), 0, "查看历史不会发写请求");

for (const theme of ["light", "dark"]) {
  ev(`document.documentElement.dataset.theme = ${JSON.stringify(theme)}`);
  run("set", "viewport", "420", "820"); wait();
  assert.equal(ev("(() => { const section=document.querySelector('.bug-history'); const dialog=section.closest('dialog'); return section.scrollWidth <= section.clientWidth + 1 && dialog.scrollWidth <= dialog.clientWidth + 1; })()"), true, `${theme} 窄屏历史不横向溢出`);
}
run("set", "viewport", "1280", "900");
open("fail");
assert.match(text(), /历史记录加载失败/);
ev("window.__bugHistoryMode = ''"); clickText("重新加载历史"); wait();
assert.equal(ev("window.__bugHistoryCalls.length"), 2);
assert.equal(ev("document.querySelectorAll('.bug-history-entry').length"), 3);
open("empty");
assert.match(text(), /暂无历史记录/);
assert.match(text(), /未找到指派记录/);

open("pending");
assert.match(text(), /正在读取禅道历史记录/);
close();
ev("window.__bugHistoryMode = ''"); run("click", ".bugs-title"); wait();
ev("window.__bugHistoryPending[0]({assignedBy:'过期指派人',assignedAt:null,createdBy:null,entries:[]})"); wait();
assert.equal(text().includes("过期指派人"), false, "关闭后延迟请求不能覆盖重开的详情");
assert.match(text(), /吴敏政/);

console.log(JSON.stringify({ history: true, attribution: true, order: true, changes: true, safeHtml: true, noRemoteImages: true, readOnly: true, retry: true, empty: true, pending: true, staleResponse: true, themes: true, narrowViewport: true }));
