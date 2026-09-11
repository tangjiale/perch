(() => {
  window.appCategories = ['开发工具', '协作办公', '运维监控'];
  let suspended = null, returnFocus = null, previousFocus = null, pendingDelete = null;
  const options = selected => `<option value="" ${!selected?'selected':''}>未分类</option>${window.appCategories.map(name=>`<option value="${esc(name)}" ${name===selected?'selected':''}>${esc(name)}</option>`).join('')}`;
  const used = name => window.demoApps.filter(app=>app.category===name).length;
  const draftSelect = () => suspended?.querySelector('#app-category');
  const error = message => { document.getElementById('category-error').textContent = message; };

  function validate(name, original) {
    if (!name) return '请输入分类名称';
    if (name.length > 30) return '分类名称最多 30 个字符';
    if (['全部','收藏','未分类'].includes(name)) return '此名称为系统筛选项，请使用其他名称';
    if (window.appCategories.some(item=>item!==original&&item.toLocaleLowerCase()===name.toLocaleLowerCase())) return '分类名称已存在';
    return '';
  }

  function refreshSelection(oldName, newName) {
    const select = draftSelect();
    if (select) {
      const value = select.value === oldName ? newName : select.value;
      select.innerHTML = options(value);
      select.value = value;
    }
  }

  function renderList() {
    document.getElementById('category-list').innerHTML = window.appCategories.map(name=>`<div class="category-entry">
      <form class="category-row" data-category-original="${esc(name)}">
        <input class="field-input" name="name" value="${esc(name)}" maxlength="30" required aria-label="分类名称：${esc(name)}">
        <span class="category-usage">${used(name)} 个应用</span>
        <button class="icon-btn" type="submit" title="保存分类名称" aria-label="保存 ${esc(name)} 的名称">${ico('check')}</button>
        <button class="icon-btn" type="button" data-category-action="delete" data-category="${esc(name)}" title="删除分类" aria-label="删除 ${esc(name)}">${ico('trash-2')}</button>
      </form>
      ${pendingDelete===name?`<div class="category-delete-confirm"><p>删除「${esc(name)}」？${used(name)?` ${used(name)} 个应用需要调整分类。`:''}${draftSelect()?.value===name?' 当前应用表单正在使用此分类。':''}</p>
        ${used(name)||draftSelect()?.value===name?`<label for="category-replacement">移至分类</label><select class="field-input" id="category-replacement"><option value="" disabled selected>请选择替代分类</option><option value="none">未分类</option>${window.appCategories.map((item,index)=>item!==name?`<option value="category-${index}">${esc(item)}</option>`:'').join('')}</select>`:''}
        <div class="category-delete-actions"><button type="button" class="btn" data-category-action="cancel-delete">取消</button><button type="button" class="btn danger" data-category-action="confirm-delete" data-category="${esc(name)}">删除分类</button></div></div>`:''}
      </div>`).join('') || '<p class="category-empty">暂无分类</p>';
    icons();
  }

  function open() {
    if (suspended) return;
    const root = document.getElementById('overlay-root');
    returnFocus = document.activeElement;
    previousFocus = lastFocus;
    // 保留真实表单节点，返回时输入值和 FileList 都不会丢失。
    suspended = document.createDocumentFragment();
    suspended.append(...root.childNodes);
    pendingDelete = null;
    openOverlay(`<section class="modal category-manager" role="dialog" aria-modal="true" aria-labelledby="category-manager-title">
      <div class="category-manager-heading"><h2 id="category-manager-title">管理分类</h2><button type="button" class="icon-btn" data-action="close" title="关闭分类管理" aria-label="关闭分类管理">${ico('x')}</button></div>
      <div id="category-list"></div>
      <form id="category-new-form" class="category-new-form"><label class="sr-only" for="category-new-name">新分类名称</label><input class="field-input" id="category-new-name" name="name" maxlength="30" required placeholder="新分类名称"><button class="btn" type="submit">${ico('plus')}添加</button></form>
      <div class="field-error" id="category-error" role="alert"></div>
      <div class="modal-actions"><button class="btn primary" type="button" data-action="close">完成</button></div>
    </section>`, true);
    renderList();
  }

  function close() {
    if (!suspended) return;
    document.getElementById('overlay-root').replaceChildren(suspended);
    suspended = null;
    pendingDelete = null;
    lastFocus = previousFocus;
    render();
    returnFocus?.focus();
  }

  document.addEventListener('click', event=>{
    const button = event.target.closest('[data-category-action]');
    if (!button) return;
    const {categoryAction:action, category:name} = button.dataset;
    if (action==='open') { open(); return; }
    if (!suspended) return;
    error('');
    if (action==='delete') { pendingDelete=name; renderList(); }
    if (action==='cancel-delete') { pendingDelete=null; renderList(); }
    if (action==='confirm-delete') {
      if (pendingDelete!==name || !window.appCategories.includes(name)) return;
      const replacement = document.getElementById('category-replacement');
      if ((used(name)||draftSelect()?.value===name) && !replacement?.value) { error('请选择应用要移入的分类'); return; }
      const value = replacement?.value || 'none';
      const target = value==='none' ? '' : window.appCategories[Number(value.replace('category-',''))];
      if (target===undefined || target===name) { error('请选择有效的替代分类'); return; }
      window.demoApps.forEach(app=>{if(app.category===name)app.category=target;});
      window.appCategories = window.appCategories.filter(item=>item!==name);
      if (window.appCategory===name) window.appCategory=target||(window.demoApps.some(app=>!app.category)?'未分类':'全部');
      refreshSelection(name,target);
      pendingDelete=null;
      renderList(); render(); showToast('分类已删除');
    }
  });

  document.addEventListener('submit', event=>{
    const form = event.target;
    if (form.id!=='category-new-form' && !form.matches('.category-row')) return;
    event.preventDefault();
    const original = form.dataset.categoryOriginal;
    const name = String(new FormData(form).get('name')||'').trim();
    const message = validate(name,original);
    if (message) { error(message); return; }
    if (original!==undefined) {
      const index=window.appCategories.indexOf(original);
      if (index<0) return;
      window.appCategories[index]=name;
      window.demoApps.forEach(app=>{if(app.category===original)app.category=name;});
      if (window.appCategory===original) window.appCategory=name;
      refreshSelection(original,name);
    } else {
      window.appCategories.push(name);
      refreshSelection(null,null);
      form.reset();
    }
    pendingDelete=null;
    error(''); renderList(); render(); showToast(original!==undefined?'分类名称已保存':'分类已添加');
  });
  window.categoryManager = {open,close,isOpen:()=>suspended!==null,options};
})();
