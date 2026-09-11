(() => {
  const store = window.resourceStore;
  const extensions = ['ppt','pptx','doc','docx','xls','xlsx','txt','md','pdf'];
  const maxSize = 30 * 1024 * 1024;
  let selectedId = null;
  let uploadMessage = '';
  const find = id => store.findKnowledge(id);
  const btn = (action, id, icon, title, classes = 'icon-btn') => `<button type="button" class="${classes}" data-knowledge-action="${action}" data-id="${esc(id || '')}" title="${esc(title)}" aria-label="${esc(title)}">${ico(icon)}${classes.includes('btn ') ? esc(title) : ''}</button>`;
  const bytes = size => size >= 1024 * 1024 ? `${(size / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.ceil(size / 1024))} KB`;
  const modelName = kb => { const {provider,model,available} = store.resolveEmbedding(kb); return `<span class="knowledge-model ${available ? '' : 'unavailable'}">${ico('cpu')}<span>${esc(model?.name || model?.modelId || '模型已不可用')}<small>${esc(provider?.name || '供应商不可用')}${available ? '' : ' · 未启用'}</small></span></span>`; };
  const referenced = id => window.aiStore.conversations.some(conversation => conversation.knowledgeBaseId === id || conversation.messages.some(message => message.knowledge?.id === id));

  function renderPage() {
    const kb = find(selectedId);
    if (kb) return detailPage(kb);
    selectedId = null;
    const query = String(window.searchTerm || '').trim().toLowerCase();
    const visible = store.knowledgeBases.filter(item => `${item.name} ${item.description}`.toLowerCase().includes(query));
    return `<div class="knowledge-page"><div class="page-heading"><div><h1>知识库</h1><p>${store.knowledgeBases.length} 个知识库 · ${store.knowledgeBases.reduce((total,item) => total + item.documents.length, 0)} 份文档</p></div>${btn('new','','plus','新建知识库','btn primary')}</div><div class="knowledge-list" role="table" aria-label="知识库列表"><div class="knowledge-list-head" role="row"><span role="columnheader">知识库</span><span role="columnheader">向量模型</span><span role="columnheader">文档</span><span role="columnheader">状态</span><span role="columnheader">操作</span></div>${visible.map(item => `<div class="knowledge-list-row" role="row"><div class="knowledge-identity" role="cell"><span class="knowledge-symbol">${ico('library')}</span><div><button class="knowledge-name" data-knowledge-action="detail" data-id="${esc(item.id)}">${esc(item.name)}</button><p>${esc(item.description || '暂无描述')}</p></div></div><div role="cell">${modelName(item)}</div><span class="knowledge-document-count" role="cell">${item.documents.length} 份</span><span class="knowledge-state" role="cell">${ico('circle-dashed')}${item.documents.length ? '待解析' : '暂无文档'}</span><div class="knowledge-row-actions" role="cell">${btn('edit',item.id,'square-pen','编辑知识库')}${btn('detail',item.id,'arrow-up-right','管理文档')}</div></div>`).join('') || `<div class="knowledge-empty">${ico('library')}<h2>${query ? '没有匹配的知识库' : '暂无知识库'}</h2>${query ? '' : btn('new','','plus','新建知识库','btn primary')}</div>`}</div><div class="knowledge-footer">${ico('hard-drive')}文档解析与向量检索尚未接入</div></div>`;
  }

  function detailPage(kb) {
    const query = String(window.searchTerm || '').trim().toLowerCase();
    const docs = kb.documents.filter(doc => doc.name.toLowerCase().includes(query));
    const embedding = store.resolveEmbedding(kb);
    return `<div class="knowledge-page knowledge-detail"><button class="knowledge-back" data-knowledge-action="back">${ico('arrow-left')}全部知识库</button><div class="page-heading"><div><h1>${esc(kb.name)}</h1><p>${esc(kb.description || '暂无描述')}</p></div><div class="knowledge-heading-actions">${btn('edit',kb.id,'settings-2','知识库设置','btn secondary')}${btn('upload',kb.id,'upload','上传文件','btn primary')}</div></div><div class="knowledge-detail-meta">${modelName(kb)}<span>${ico('files')}${kb.documents.length} 份文档</span><span class="knowledge-state">${ico('circle-dashed')}待接入解析服务</span></div>${!embedding.available ? '<div class="knowledge-warning">向量模型当前不可用。<a href="#models">前往模型配置</a></div>' : ''}<input id="knowledge-file-input" type="file" multiple accept="${extensions.map(ext => `.${ext}`).join(',')}" data-knowledge-id="${esc(kb.id)}" hidden><div class="knowledge-dropzone ${kb.documents.length ? 'compact' : ''}" data-knowledge-drop="${esc(kb.id)}"><span class="knowledge-upload-icon">${ico('cloud-upload')}</span><div><strong>拖拽文件到此处</strong><p>PPT、Word、Excel、TXT、MD、PDF · 单个文件不超过 30 MB</p></div>${btn('upload',kb.id,'plus','选择文件','btn secondary')}</div><div class="knowledge-upload-result" role="status" aria-live="polite">${esc(uploadMessage)}</div><div class="knowledge-documents-heading"><h2>文档 <span>${kb.documents.length}</span></h2><span>待解析 ${kb.documents.length}</span></div>${kb.documents.length ? `<div class="knowledge-doc-list" role="table" aria-label="知识库文档"><div class="knowledge-doc-head" role="row"><span role="columnheader">文件名称</span><span role="columnheader">大小</span><span role="columnheader">添加日期</span><span role="columnheader">状态</span><span role="columnheader">操作</span></div>${docs.map(doc => `<div class="knowledge-doc-row" role="row"><div class="knowledge-file-name" role="cell"><span class="knowledge-file-icon ${['xls','xlsx'].includes(doc.type) ? 'sheet' : ['ppt','pptx','pdf'].includes(doc.type) ? 'presentation' : ''}">${ico(['xls','xlsx'].includes(doc.type) ? 'file-spreadsheet' : ['ppt','pptx'].includes(doc.type) ? 'presentation' : 'file-text')}</span><div><strong>${esc(doc.name)}</strong><small>${esc(doc.type.toUpperCase())}</small></div></div><span role="cell">${bytes(doc.size)}</span><time role="cell">${esc(doc.addedAt.slice(0,10))}</time><span class="knowledge-state" role="cell">${ico('circle-dashed')}待解析</span><div class="knowledge-file-actions" role="cell">${btn('download-file',doc.id,'download','下载原文件')}${btn('remove-file',doc.id,'trash-2','移除文件')}</div></div>`).join('') || '<div class="empty">没有匹配的文档</div>'}</div>` : '<div class="knowledge-doc-empty">暂无文档</div>'}<div class="knowledge-footer">${ico('hard-drive')}文件仅保留在本次预览中，尚未上传、解析或向量化。</div></div>`;
  }

  function openDetail(id) {
    if (!find(id)) return;
    selectedId = id;
    uploadMessage = '';
    window.searchTerm = '';
    document.getElementById('global-search').value = '';
    if (location.hash !== '#knowledge') location.hash = 'knowledge';
    render();
  }

  function openForm(id) {
    const kb = id ? find(id) : null;
    if (id && !kb) return;
    const models = store.embeddingModels();
    const currentValue = kb ? JSON.stringify([kb.providerId,kb.modelId]) : '';
    const missing = kb && !store.resolveEmbedding(kb).available;
    openOverlay(`<section class="modal knowledge-modal" role="dialog" aria-modal="true" aria-labelledby="knowledge-form-title"><div class="knowledge-modal-heading"><h2 id="knowledge-form-title">${kb ? '编辑知识库' : '新建知识库'}</h2>${btn('close','','x','关闭')}</div><form id="knowledge-form" data-id="${esc(id || '')}"><label for="knowledge-name">名称</label><input id="knowledge-name" class="field-input" name="name" required maxlength="60" value="${esc(kb?.name || '')}" placeholder="例如：产品资料"><label for="knowledge-description">描述</label><textarea id="knowledge-description" class="field-input" name="description" rows="3" maxlength="300" placeholder="文档内容与用途">${esc(kb?.description || '')}</textarea><label for="knowledge-model">向量模型</label><select id="knowledge-model" class="field-input" name="embedding" required ${models.length ? '' : 'disabled'}>${missing ? '<option value="" selected disabled>当前模型不可用，请重新选择</option>' : ''}${!models.length ? '<option value="">暂无可用向量模型</option>' : ''}${models.map(({provider,model}) => { const value=JSON.stringify([provider.id,model.id]); return `<option value="${esc(value)}" ${value === currentValue ? 'selected' : ''}>${esc(provider.name)} / ${esc(model.name || model.modelId)}</option>`; }).join('')}</select><p class="knowledge-model-help">${models.length ? '<a href="#models" data-knowledge-action="close">管理模型</a>' : '请先在模型配置中添加并启用向量模型。 <a href="#models" data-knowledge-action="close">前往配置</a>'}</p>${kb?.documents.length ? '<p class="knowledge-reindex-note">切换向量模型后，现有文档需要重新索引。</p>' : ''}<div id="knowledge-form-error" class="field-error" role="alert"></div><div class="modal-actions">${kb ? btn('delete',kb.id,'trash-2','删除知识库','btn danger') : ''}<button type="button" class="btn" data-knowledge-action="close">取消</button><button type="submit" class="btn primary" ${models.length ? '' : 'disabled'}>${kb ? '保存更改' : '创建知识库'}</button></div></form></section>`,true);
  }

  function addFiles(id, files) {
    const kb = find(id);
    if (!kb) return;
    let added = 0;
    const errors = [];
    for (const file of Array.from(files)) {
      const type = file.name.split('.').pop().toLowerCase();
      if (!extensions.includes(type)) { errors.push(`${file.name}：格式不支持`); continue; }
      if (file.size > maxSize) { errors.push(`${file.name}：超过 30 MB`); continue; }
      if (!file.size) { errors.push(`${file.name}：文件为空`); continue; }
      if (kb.documents.some(doc => doc.name === file.name && doc.size === file.size)) { errors.push(`${file.name}：文件已存在`); continue; }
      kb.documents.push({id:`doc-${crypto.randomUUID()}`,name:file.name,size:file.size,type,status:'pending',addedAt:new Date().toISOString(),file});
      added++;
    }
    uploadMessage = [added ? `已添加 ${added} 份文件，等待解析服务接入。` : '', ...errors].filter(Boolean).join(' ');
    if (selectedId === id && location.hash === '#knowledge') render();
    if (added) showToast(`已添加 ${added} 份文件，待解析`);
  }

  document.addEventListener('click', event => {
    const button = event.target.closest('[data-knowledge-action]');
    if (!button) return;
    const {knowledgeAction:action,id} = button.dataset;
    if (action === 'new') openForm();
    if (action === 'edit') openForm(id);
    if (action === 'detail') openDetail(id);
    if (action === 'back') { selectedId=null; uploadMessage=''; window.searchTerm=''; document.getElementById('global-search').value=''; render(); }
    if (action === 'close') closeOverlay();
    if (action === 'upload') document.getElementById('knowledge-file-input')?.click();
    if (action === 'delete') {
      const kb=find(id); if (!kb) return;
      if (referenced(id)) return showToast('此知识库已关联聊天，请先移除相关会话后再删除');
      openOverlay(`<section class="modal knowledge-modal" role="dialog" aria-modal="true" aria-labelledby="knowledge-delete-title"><h2 id="knowledge-delete-title">删除知识库</h2><p>确认删除「${esc(kb.name)}」及库内 ${kb.documents.length} 份文档？原始文件不受影响。</p><div class="modal-actions"><button class="btn" data-knowledge-action="edit" data-id="${esc(id)}">取消</button><button class="btn danger" data-knowledge-action="confirm-delete" data-id="${esc(id)}">删除知识库</button></div></section>`,true);
    }
    if (action === 'confirm-delete') {
      if (referenced(id)) return showToast('此知识库已关联聊天，无法删除');
      store.knowledgeBases=store.knowledgeBases.filter(kb => kb.id !== id);
      if (selectedId === id) selectedId=null;
      closeOverlay(); render(); showToast('知识库已删除');
    }
    if (action === 'download-file') {
      const doc=find(selectedId)?.documents.find(item => item.id === id);
      if (!(doc?.file instanceof File)) return showToast('原文件已不可用，请重新添加文件');
      const url=URL.createObjectURL(doc.file),link=document.createElement('a');
      link.href=url; link.download=doc.name; link.hidden=true;
      document.body.appendChild(link);
      try { link.click(); }
      finally { link.remove(); setTimeout(() => URL.revokeObjectURL(url), 60000); }
    }
    if (action === 'remove-file') {
      const kb=find(selectedId),doc=kb?.documents.find(item => item.id === id); if (!doc) return;
      openOverlay(`<section class="modal knowledge-modal" role="dialog" aria-modal="true" aria-labelledby="knowledge-remove-title"><h2 id="knowledge-remove-title">移除文件</h2><p>确认从「${esc(kb.name)}」移除 ${esc(doc.name)}？原始文件不受影响。</p><div class="modal-actions"><button class="btn" data-knowledge-action="close">取消</button><button class="btn danger" data-knowledge-action="confirm-remove-file" data-id="${esc(id)}" data-kb="${esc(kb.id)}">移除文件</button></div></section>`,true);
    }
    if (action === 'confirm-remove-file') {
      const kb=find(button.dataset.kb); if (!kb) return closeOverlay();
      kb.documents=kb.documents.filter(doc => doc.id !== id); uploadMessage='';
      closeOverlay(); render(); showToast('文件已移除');
    }
  });

  document.addEventListener('submit', event => {
    if (event.target.id !== 'knowledge-form') return;
    event.preventDefault();
    const form=event.target,data=new FormData(form),id=form.dataset.id,kb=id ? find(id) : null;
    if (id && !kb) return;
    const error=document.getElementById('knowledge-form-error'),name=String(data.get('name') || '').trim(),description=String(data.get('description') || '').trim();
    if (!name) { error.textContent='请输入知识库名称'; return; }
    if (name.length > 60) { error.textContent='知识库名称不能超过 60 个字符'; return; }
    if (description.length > 300) { error.textContent='知识库描述不能超过 300 个字符'; return; }
    if (store.knowledgeBases.some(item => item.id !== id && item.name.toLowerCase() === name.toLowerCase())) { error.textContent='此名称已存在，请使用其他名称'; return; }
    const selected=store.embeddingModels().find(({provider,model}) => JSON.stringify([provider.id,model.id]) === data.get('embedding'));
    if (!selected) { error.textContent='请选择已启用的向量模型'; return; }
    const value={name,description,providerId:selected.provider.id,modelId:selected.model.id};
    if (kb) {
      if (kb.providerId !== value.providerId || kb.modelId !== value.modelId) kb.documents.forEach(doc => { doc.status='pending'; });
      Object.assign(kb,value);
    } else store.knowledgeBases.push({id:`kb-${crypto.randomUUID()}`,...value,documents:[]});
    closeOverlay(); render(); showToast(kb ? '知识库已更新' : '知识库已创建');
  });
  document.addEventListener('change', event => {
    if (event.target.id !== 'knowledge-file-input') return;
    const input=event.target;
    addFiles(input.dataset.knowledgeId,input.files);
    input.value='';
  });
  document.addEventListener('dragover', event => {
    const zone=event.target.closest('[data-knowledge-drop]'); if (!zone) return;
    event.preventDefault(); zone.classList.add('dragging');
  });
  document.addEventListener('dragleave', event => {
    const zone=event.target.closest('[data-knowledge-drop]');
    if (zone && !zone.contains(event.relatedTarget)) zone.classList.remove('dragging');
  });
  document.addEventListener('drop', event => {
    const zone=event.target.closest('[data-knowledge-drop]'); if (!zone) return;
    event.preventDefault(); zone.classList.remove('dragging');
    addFiles(zone.dataset.knowledgeDrop,event.dataTransfer.files);
  });
  window.knowledgePages={render:renderPage,openDetail};
})();
