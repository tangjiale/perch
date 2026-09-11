(async () => {
  const { emptySnapshot } = await import('/src/lib/api.ts');
  const day = new Date().toLocaleDateString('en-CA');
  const base = { notes: '', priority: 'normal', status: 'todo', sortOrder: 1 };
  emptySnapshot.tasks = [
    { ...base, id: 'visual-local', title: '界面测试 · 设计评审', source: 'local', schedule: { kind: 'timed', timezone: 'Asia/Shanghai', start: `${day}T09:00:00+08:00`, end: `${day}T10:00:00+08:00` } },
    { ...base, id: 'visual-remote', title: '界面测试 · 禅道执行', source: 'zentao', schedule: { kind: 'timed', timezone: 'Asia/Shanghai', start: `${day}T11:00:00+08:00`, end: `${day}T12:00:00+08:00` } },
    { ...base, id: 'visual-unscheduled', title: '界面测试 · 未排期', source: 'local' },
  ];
  const pause = () => new Promise(resolve => setTimeout(resolve, 300));
  location.hash = '/apps'; await pause(); location.hash = '/calendar'; await pause();
  const results = [];
  const assert = (name, value) => { results.push({ name, passed: !!value }); if (!value) throw Error(name); };
  const press = async label => { const button=[...document.querySelectorAll('.wb-calendar button')].find(b=>b.textContent===label); if(!button)throw Error(label);button.click();await pause(); };
  assert('默认月视图', document.querySelector('.fc-dayGridMonth-view'));
  assert('两个任务显示', document.querySelectorAll('.fc-daygrid-event').length===2);
  assert('导航为 SVG 图标',document.querySelectorAll('.wb-calendar-icon svg').length===4);
  assert('未排期入口',document.querySelector('.wb-calendar-unscheduled').textContent.includes('界面测试 · 未排期'));
  const local=document.querySelector('.wb-calendar-sources input');local.click();await pause();
  assert('取消个人来源后仅显示禅道',document.querySelectorAll('.fc-daygrid-event').length===1 && document.querySelector('.fc-daygrid-event').textContent.includes('禅道'));
  local.click();await pause();
  await press('日');assert('日视图切换',document.querySelector('.fc-timeGridDay-view'));
  assert('选中态为深色文字',getComputedStyle(document.querySelector('.wb-calendar-segmented .is-active')).color==='rgb(37, 52, 74)');
  assert('全天区域没有继承月格高度',document.querySelector('.fc-timegrid .fc-daygrid-day-frame').getBoundingClientRect().height<65);
  const title=document.querySelector('.wb-calendar-navigation h2').textContent;
  document.querySelector('[aria-label="下一时段"]').click();await pause();assert('下一时段生效',document.querySelector('.wb-calendar-navigation h2').textContent!==title);
  await press('今天');assert('今天返回当前日期',document.querySelector('.wb-calendar-navigation h2').textContent===title);
  await press('周');assert('周视图切换',document.querySelector('.fc-timeGridWeek-view'));
  await press('列表');assert('列表切换',document.querySelector('.fc-listMonth-view'));
  await press('月');assert('回到月视图',document.querySelector('.fc-dayGridMonth-view'));
  assert('页面无横向溢出',document.documentElement.scrollWidth<=innerWidth);
  return results;
})()
