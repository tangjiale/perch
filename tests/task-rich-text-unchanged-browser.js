(async () => {
  if(location.pathname !== '/tests/task-rich-text.html' || !location.search.includes('retry')) throw Error('仅允许隔离富文本重试测试');
  const wait=()=>new Promise(resolve=>setTimeout(resolve,250));
  const assert=(value,message)=>{if(!value)throw Error(message);};
  await wait();
  const original=document.getElementById('root').dataset.originalNotes;
  document.querySelector('.task-rich-image img').click();await wait();
  const viewer=document.querySelector('dialog.image-viewer[open]');
  assert(viewer,'点击备注图片应打开看图器');
  viewer.querySelector('[aria-label="放大图片"]').click();await wait();
  viewer.dispatchEvent(new Event('cancel',{cancelable:true,bubbles:true}));await wait();
  assert(document.querySelector('dialog.image-viewer[open]'),'Esc 不应关闭看图器');
  viewer.querySelector('button[title="关闭"]').click();await wait();
  assert(!document.querySelector('dialog.image-viewer[open]'),'右上角关闭看图器应回到编辑表单');
  assert(!window.richCalls.some(call=>call.name==='save_task'),'预览不得触发保存');
  document.querySelector('[role=combobox][aria-label="任务状态"]').click();await wait();
  [...document.querySelectorAll('[role=option]')].find(option=>option.textContent.includes('已完成')).click();await wait();
  for(let attempt=0;attempt<2;attempt++) {
    document.querySelector('dialog form').requestSubmit();await wait();
    const saved=window.richCalls.findLast(call=>call.name==='save_task');
    assert(saved?.args.value.status==='done','状态修改应正常提交');
    assert(saved.args.value.notes===original,'初始化和保存失败解除禁用都不能改写未编辑的备注');
  }
  assert(window.richCalls.filter(call=>call.name==='save_task').length===2,'应完成两次保存尝试');
  const area=document.querySelector('.tiptap');
  area.focus();const selection=window.getSelection();selection.selectAllChildren(area);selection.collapseToEnd();
  assert(document.execCommand('insertText',false,'预览后仍可编辑备注'),'应能继续输入文字');await wait();
  document.querySelector('dialog form').requestSubmit();await wait();
  const edited=window.richCalls.findLast(call=>call.name==='save_task').args.value.notes;
  assert(edited.includes('预览后仍可编辑备注') && edited.includes('{708.png}') && !edited.includes('data:image'),'真实编辑应保存正文并保留图片引用');
  return {statusOnlyPreservesNotes:true,failedSavePreservesNotes:true,imagePreviewDoesNotEditNotes:true,editingAfterPreview:true};
})();
