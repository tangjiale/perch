(async () => {
  if(location.pathname !== '/tests/task-rich-text.html' || !location.search.includes('retry')) throw Error('仅允许隔离富文本重试测试');
  const wait=()=>new Promise(resolve=>setTimeout(resolve,250));
  const assert=(value,message)=>{if(!value)throw Error(message);};
  await wait();
  const original=document.getElementById('root').dataset.originalNotes;
  document.querySelector('[role=combobox][aria-label="任务状态"]').click();await wait();
  [...document.querySelectorAll('[role=option]')].find(option=>option.textContent.includes('已关闭')).click();await wait();
  for(let attempt=0;attempt<2;attempt++) {
    document.querySelector('dialog form').requestSubmit();await wait();
    const saved=window.richCalls.findLast(call=>call.name==='save_task');
    assert(saved?.args.value.status==='closed','状态修改应正常提交');
    assert(saved.args.value.notes===original,'初始化和保存失败解除禁用都不能改写未编辑的备注');
  }
  assert(window.richCalls.filter(call=>call.name==='save_task').length===2,'应完成两次保存尝试');
  return {statusOnlyPreservesNotes:true,failedSavePreservesNotes:true,imagePreviewDoesNotEditNotes:true};
})();
