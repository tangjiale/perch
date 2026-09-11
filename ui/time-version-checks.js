// 在 agent-browser eval --stdin 中运行；仅修改当前预览会话，运行后刷新恢复示例。
(async () => {
  const results = [];
  const check = (name, condition) => {
    if (!condition) throw new Error(name);
    results.push(name);
  };
  const wait = (ms = 30) => new Promise(resolve => setTimeout(resolve, ms));
  const fill = (selector, value) => {
    const element = document.querySelector(selector);
    if (!element) throw new Error(`未找到 ${selector}`);
    element.value = value;
  };
  const click = selector => {
    const element = document.querySelector(selector);
    if (!element) throw new Error(`未找到 ${selector}`);
    element.click();
  };
  const go = async page => { closeOverlay(); navigate(page); await wait(); };

  check('品牌显示应用版本', document.querySelector('#brand-version').textContent === 'v0.1.0');
  const original = projectTaskAPI.getTasks().find(task => task.id === 'T-101');
  projectTaskAPI.openTask(original.id);
  check('详情回显结束时间', document.querySelector('#detail-end').value === '10:00');
  fill('#detail-end', '08:30');
  click('[data-action="save-detail"]');
  check('结束早于开始被拦截', !!document.querySelector('#detail-form') && document.querySelector('#detail-error').textContent.includes('晚于'));
  check('校验失败保留输入', document.querySelector('#detail-end').value === '08:30');
  fill('#detail-end', '10:30');
  click('[data-action="save-detail"]');
  check('有效结束时间已保存', original.end === '10:30');
  projectTaskAPI.openTask(original.id);
  check('重新打开仍回显结束时间', document.querySelector('#detail-end').value === '10:30');
  fill('#detail-date', '');
  click('[data-action="save-detail"]');
  check('有时间时不能清空日期', document.querySelector('#detail-error').textContent.includes('计划日期') && original.date === '2026-09-07');
  closeOverlay();

  const remote = projectTaskAPI.getTasks().find(task => task.id === 'E-238');
  const remoteState = {remote: remote.remote, projectId: remote.projectId};
  projectTaskAPI.openTask(remote.id);
  fill('#detail-end', '12:30');
  click('[data-action="save-detail"]');
  check('禅道项只更新个人结束时间', remote.end === '12:30' && remote.remote === remoteState.remote && remote.projectId === remoteState.projectId);

  newTask('todo');
  fill('#new-title-input', '结束时间回归任务');
  fill('#new-time', '13:00');
  fill('#new-end', '14:15');
  document.querySelector('#new-task-form').requestSubmit();
  const created = projectTaskAPI.getTasks().at(-1);
  check('新建任务保存开始和结束时间', created.time === '13:00' && created.end === '14:15');

  newTask('todo');
  fill('#new-title-input', '无开始时间任务');
  fill('#new-end', '15:00');
  document.querySelector('#new-task-form').requestSubmit();
  check('只有结束时间被拦截', document.querySelector('#new-task-error').textContent.includes('开始时间'));
  check('无效新建不写入任务', projectTaskAPI.getTasks().at(-1) === created);
  closeOverlay();

  await go('overview');
  check('总览显示时间段', [...document.querySelectorAll('.task-row')].some(row => row.textContent.includes('09:00–10:30')));
  await go('tasks');
  check('看板显示时间段', document.querySelector(`[data-task-id="${created.id}"]`).textContent.includes('13:00–14:15'));
  click('[data-action="task-view"][data-value="list"]');
  check('列表显示时间段', [...document.querySelectorAll('.task-table tbody tr')].some(row => row.textContent.includes('13:00–14:15')));
  await go('calendar');
  check('月历显示时间段', [...document.querySelectorAll('.calendar-event')].some(event => event.textContent.includes('09:00–10:30')));
  click('[data-action="calendar-mode"][data-value="week"]');
  check('周历显示时间段', [...document.querySelectorAll('.calendar-event')].some(event => event.textContent.includes('13:00–14:15')));
  check('全天任务不显示空分隔符', ![...document.querySelectorAll('.calendar-event')].some(event => event.textContent.includes('– ')));

  return {passed: results.length, results};
})()
