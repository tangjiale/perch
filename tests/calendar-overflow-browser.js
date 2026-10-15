// 隔离生产日历组件，不读取原生工作空间或远端数据。
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
const origin = process.argv[2] || 'http://127.0.0.1:1422';
const run = (...args) => execFileSync('npx', ['--yes', 'agent-browser', '--session', 'calendar-overflow-test', ...args], { encoding: 'utf8', timeout: 60000 });
const ev = code => JSON.parse(JSON.parse(run('eval', `JSON.stringify(${code})`).trim()));
const wait = () => run('wait', '250');
const titles = () => ev(`(()=>{const date=document.querySelector('[data-target-date]').dataset.targetDate;return [...document.querySelectorAll('.fc-daygrid-day[data-date="'+date+'"] .fc-daygrid-event-harness')].filter(e=>e.getBoundingClientRect().height>0&&getComputedStyle(e).visibility!=='hidden').map(e=>e.querySelector('.wb-calendar-event-title').textContent)})()`);
try {
  run('set', 'viewport', '1440', '1000');
  run('open', `${origin}/tests/calendar-overflow.html`);
  wait();
  assert.deepEqual(titles(), ['未完成早开始', '进行中当天', '未完成当天'], '未完成优先于早开始长跨天完成任务，组内保留时间和时长排序');
  const more = ev(`(()=>{const date=document.querySelector('[data-target-date]').dataset.targetDate;const link=document.querySelector('.fc-daygrid-day[data-date="'+date+'"] .fc-daygrid-more-link');link.dataset.overflowTarget='yes';return link.textContent})()`);
  assert.equal(more, '+2 项', '完成任务优先进入折叠');
  run('click', '[data-overflow-target=yes]');
  wait();
  const expanded = ev(`[...document.querySelectorAll('.fc-more-popover .wb-calendar-event-title')].map(e=>e.textContent)`);
  assert.ok(expanded.includes('已完成长跨天') && expanded.includes('已关闭长跨天'), '展开仍可查看完成和关闭任务');
  run('click', '.fc-popover-close');
  run('click', '[data-scenario=few]');
  wait();
  assert.deepEqual(titles(), ['未完成当天', '已完成长跨天', '已关闭长跨天'], '有空间时正常展示完成任务，同状态保持时间顺序');
  run('click', '[data-scenario=complete]');
  wait();
  assert.deepEqual(titles().slice(0, 2), ['进行中当天', '未完成当天'], '完成状态变化立即重新排序');
  assert.equal(titles().length, 3, '完成后有空位仍展示一项完成任务');
  run('click', '[data-scenario=crowded]');
  wait();
  ev(`document.documentElement.dataset.theme='light'`);
  run('screenshot', '/tmp/perch-calendar-overflow-light.png');
  ev(`document.documentElement.dataset.theme='dark'`);
  run('screenshot', '/tmp/perch-calendar-overflow-dark.png');
  console.log('日历折叠优先级 7 项浏览器验收通过（浅色/深色截图已保存）');
} finally { run('close'); }
