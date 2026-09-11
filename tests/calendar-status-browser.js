(async()=>{
 if(location.pathname!='/tests/calendar-status.html')throw Error('仅允许隔离验证');
 const pause=()=>new Promise(resolve=>setTimeout(resolve,250));
 const results=[];await pause();
 for(const theme of ['light','dark']){
  document.documentElement.dataset.theme=theme;
  for(const label of document.querySelectorAll('.wb-calendar-sources label')) {
   const checkbox=label.querySelector('input'),dot=label.querySelector('.wb-calendar-source-dot');
   if(getComputedStyle(checkbox).accentColor!==getComputedStyle(dot).backgroundColor)throw Error(theme+'来源颜色不一致');
  }
  results.push(theme+'：三个来源勾选框与圆点颜色一致');
  for(const view of ['日','周','月','列表']){
   [...document.querySelectorAll('.wb-calendar-segmented button')].find(b=>b.textContent===view).click();await pause();
   const events=[...document.querySelectorAll('.fc-event')];
   for(const title of ['已完成禅道执行','已关闭禅道执行']){
    const event=events.find(e=>e.textContent.includes(title));
    const text=event?.querySelector('.wb-calendar-event-title');
    if(!text||!event.classList.contains('wb-calendar-event-done')||getComputedStyle(text).textDecorationLine!=='line-through')throw Error(theme+view+title+'缺少灰色删除线');
    const expected=theme==='dark'?'rgb(169, 175, 184)':'rgb(116, 123, 133)';
    if(getComputedStyle(text).color!==expected)throw Error('状态颜色错误');
   }
   const active=events.find(e=>e.textContent.includes('进行中禅道执行'));
   if(!active||active.classList.contains('wb-calendar-event-done'))throw Error('进行中任务被误标');
   results.push(theme+' '+view+'：完成和关闭均显示灰色删除线，进行中保持原样');
  }
 }
 return results;
})();
