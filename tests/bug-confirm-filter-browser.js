// 仅模拟 BUG fixture，不连接真实禅道。
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";
const origin = process.argv[2] || "http://127.0.0.1:1421";
const run = (...args) =>
  execFileSync(
    "npx",
    ["--yes", "agent-browser", "--session", "bug-confirm-filter", ...args],
    { encoding: "utf8", timeout: 60000 },
  );
const ev = (code) =>
  JSON.parse(JSON.parse(run("eval", `JSON.stringify(${code})`).trim()));
const wait = () => run("wait", "200");
const select = (label, option) => {
  run("click", `[aria-label="${label}"][role=combobox]`);
  ev(
    `(() => { const option=[...document.querySelectorAll('[role=option]')].find(e=>e.textContent.trim()===${JSON.stringify(option)}); if(!option) throw Error('缺少选项');option.dataset.testSelection='chosen';return true; })()`,
  );
  run("click", '[data-test-selection="chosen"]');
  wait();
};
const paging = () =>
  ev(`document.querySelector('.bugs-pagination').textContent`);
const table = () => ev(`document.querySelector('.bugs-table').textContent`);
try {
  run("open", `${origin}/tests/bugs.html?confirmationFilter=1&status=all`);
  wait();
  assert.match(
    ev(`document.querySelector('[aria-label="是否确认"]').textContent`),
    /全部确认状态/,
  );
  assert.match(paging(), /共 122 个 BUG/);
  ev(`document.querySelector('.bugs-pagination button:last-child').click()`);
  wait();
  assert.match(paging(), /第 2 \/ 3/);
  select("是否确认", "已确认");
  assert.match(paging(), /共 60 个 BUG.*第 1 \/ 2/);
  assert.doesNotMatch(table(), /未确认|待同步/);
  ev(`document.querySelector('.bugs-pagination button:last-child').click()`);
  wait();
  assert.match(paging(), /第 2 \/ 2/);
  select("是否确认", "未确认");
  assert.match(paging(), /共 60 个 BUG.*第 1 \/ 2/);
  assert.doesNotMatch(table(), /已确认|待同步/);
  select("是否确认", "全部确认状态");
  run("fill", '[aria-label="搜索 BUG"]', "边界筛选");
  wait();
  assert.match(paging(), /共 3 个 BUG/);
  assert.match(table(), /待同步/);
  select("是否确认", "未确认");
  assert.equal(
    ev(`document.querySelectorAll('.bugs-table tbody tr').length`),
    0,
  ); // 12 是已确认，120/121 均为未知。
  select("是否确认", "已确认");
  run("fill", '[aria-label="搜索 BUG"]', " ");
  wait();
  ev(`document.querySelectorAll('.bugs-status-filters button')[1].click()`);
  wait();
  select("筛选 BUG 项目", "甲项目 #11（62）");
  select("筛选 BUG 严重程度", "严重程度 1");
  const expected = ev(
    `window.__fixtureBugs.filter(b=>b.confirmed===true&&b.status==='active'&&b.projectId==='11'&&b.severity===1).length`,
  );
  assert.match(paging(), new RegExp(`共 ${expected} 个 BUG`));
  assert.doesNotMatch(table(), /乙项目|未确认|待同步/);
  run("fill", '[aria-label="搜索 BUG"]', "BUG 48");
  wait();
  assert.match(paging(), /共 1 个 BUG/);
  assert.match(table(), /筛选 BUG 48/);
  run("set", "viewport", "1440", "1000");
  ev(`document.documentElement.dataset.theme='light'`);
  assert.equal(ev(`document.documentElement.scrollWidth>innerWidth`), false);
  run("screenshot", "/tmp/perch-bug-confirm-filter-light.png");
  run("set", "viewport", "900", "760");
  ev(`document.documentElement.dataset.theme='dark'`);
  assert.equal(ev(`document.documentElement.scrollWidth>innerWidth`), false);
  assert.equal(
    ev(
      `document.querySelector('[aria-label="是否确认"]').getBoundingClientRect().right>innerWidth`,
    ),
    false,
  );
  run("screenshot", "/tmp/perch-bug-confirm-filter-dark.png");
  console.log("是否确认筛选 10 项模拟浏览器验收通过");
} finally {
  run("close");
}
