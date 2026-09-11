// 仅在浏览器预览中注入模拟数据，不操作桌面数据库。
(async () => {
  const { emptySnapshot, native } = await import('/src/lib/api.ts');
  if (native) throw Error('此测试仅允许浏览器预览');
  const { DateTime } = await import('/node_modules/.vite/deps/luxon.js');
  const start = DateTime.local().startOf('day');
  const tasks = [{ id:'range-fixture',title:'三天开发任务',notes:'',priority:'normal',status:'doing',sortOrder:0,source:'zentao',schedule:{kind:'all_day',timezone:'Asia/Shanghai',start:start.toISODate(),end:start.plus({days:3}).toISODate()} }];
  const wait = () => new Promise(resolve => setTimeout(resolve,350));
  location.hash='/apps'; await wait();
  const main = await (await fetch('/src/main.tsx')).text();
  const reactUrl = main.match(/"([^"]*deps\/react\.js[^"]*)"/)[1];
  const domUrl = main.match(/"([^"]*deps\/react-dom_client\.js[^"]*)"/)[1];
  const React = (await import(reactUrl)).default;
  const {createRoot} = (await import(domUrl)).default;
  const Calendar = (await import('/src/features/Calendar.tsx')).default;
  const host=document.createElement('div'); host.style.cssText='position:fixed;inset:0;z-index:9999;background:white'; document.body.append(host);
  const root=createRoot(host); root.render(React.createElement(Calendar,{tasks,onEdit:()=>{},onCreate:()=>{},onChange:async()=>{}})); await wait();
  for (let attempt = 0; attempt < 50 && !host.querySelector('.wb-calendar-segmented'); attempt++) {
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  [...host.querySelectorAll('.wb-calendar button')].find(b=>b.textContent==='日').click(); await wait();
  const results=[];
  for(let day=0;day<4;day++) {
    const present=[...document.querySelectorAll('.fc-event')].some(e=>e.textContent.includes('三天开发任务'));
    if(present!==(day<3)) throw Error('跨日显示错误 day='+day);
    results.push({day:day+1,visible:present,passed:true});
    document.querySelector('[aria-label="下一时段"]').click(); await wait();
  }
  root.unmount(); host.remove(); return results;
})();
