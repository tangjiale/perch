(() => {
  let selectedId = 'deepseek';
  const protocols = ['openai-completions', 'openai-responses', 'anthropic-messages'];
  const types = { chat: '对话', vision: '视觉', embedding: '向量' };
  const providers = () => window.aiStore.providers;
  const selected = () => providers().find(p => p.id === selectedId) || providers()[0];
  const refs = (providerId, modelId) => window.aiStore.agents.filter(a => a.providerId === providerId && (!modelId || a.modelId === modelId));
  const knowledgeRefs = (providerId, modelId) => window.resourceStore.knowledgeBases.filter(kb=>kb.providerId===providerId&&(!modelId||kb.modelId===modelId));
  const allRefs = (providerId, modelId) => [...refs(providerId,modelId).map(a=>`Agent「${a.name}」`),...knowledgeRefs(providerId,modelId).map(kb=>`知识库「${kb.name}」`)];
  const btn = (action, icon, title, attrs = '') => `<button type="button" class="icon-btn" data-model-action="${action}" title="${esc(title)}" aria-label="${esc(title)}" ${attrs}>${ico(icon)}</button>`;
  const field = (id, label, html) => `<div class="model-field"><label for="model-${id}">${label}</label>${html}</div>`;
  const typeOptions = value => Object.entries(types).map(([id, name]) => `<option value="${id}" ${id === value ? 'selected' : ''}>${name}</option>`).join('');

  function renderPage() {
    const p = selected();
    if (p) selectedId = p.id;
    return `<div class="page-heading"><div><h1>设置</h1><p>模型供应商与连接配置</p></div><span class="tag">未连接模型服务</span></div>
      ${secondaryPages.settingsTabs('models')}
      <div class="model-layout" id="settings-panel" role="tabpanel" aria-labelledby="settings-tab-models" tabindex="0"><aside class="model-providers" aria-label="模型供应商"><div class="model-list-heading"><h2>供应商 <span>${providers().length}</span></h2>${btn('add-provider','plus','添加供应商')}</div>
      <div class="model-provider-list">${providers().map(provider => `<button type="button" class="model-provider ${p?.id === provider.id ? 'active' : ''}" data-model-action="select" data-id="${esc(provider.id)}" aria-pressed="${p?.id === provider.id}"><span class="model-provider-mark">${ico(provider.kind === 'preset' ? 'network' : 'server')}</span><span class="model-provider-copy"><strong>${esc(provider.name)}</strong><small>${provider.models.length} 个模型 · ${provider.enabled ? '已启用' : '已停用'}</small></span>${ico('chevron-right')}</button>`).join('')}</div>
      <button class="btn model-add-provider" type="button" data-model-action="add-provider">${ico('plus')}添加供应商</button></aside>
      <section class="model-config" aria-label="供应商配置">${p ? configuration(p) : '<div class="empty">暂无供应商</div>'}</section></div>`;
  }

  function configuration(p) {
    return `<div class="model-config-heading"><div class="model-config-title"><span class="model-provider-mark">${ico(p.kind === 'preset' ? 'network' : 'server')}</span><div><h2>${esc(p.name)}</h2><span class="muted">${p.kind === 'preset' ? '预设供应商' : '自定义供应商'}</span></div></div><div class="model-config-status"><span class="badge"><span class="dot"></span>未验证</span>${btn('remove-provider','trash-2','移除供应商',`data-id="${esc(p.id)}"`)}</div></div>
      <form id="model-provider-form" data-id="${esc(p.id)}" autocomplete="off">
        <div class="model-field-grid">${field('name','显示名称',`<input class="field-input" id="model-name" name="name" required maxlength="60" value="${esc(p.name)}">`)}${field('protocol','API 协议',`<select class="field-input" id="model-protocol" name="protocol">${protocols.map(protocol => `<option ${p.protocol === protocol ? 'selected' : ''}>${protocol}</option>`).join('')}</select>`)}</div>
        ${field('url','API 地址',`<input class="field-input" id="model-url" type="url" name="baseUrl" required value="${esc(p.baseUrl)}" placeholder="https://api.example.com/v1">`)}
        ${field('key','API 密钥',`<div class="model-key-field"><input class="field-input" id="model-key" type="password" autocomplete="new-password" placeholder="尚未配置" aria-describedby="model-key-note">${btn('clear-key','x','清空密钥')}</div><small id="model-key-note">本次设计预览不保存密钥</small>`)}
        <div class="model-provider-state"><label for="model-enabled"><input type="checkbox" id="model-enabled" name="enabled" ${p.enabled ? 'checked' : ''}>启用供应商</label><button class="btn" type="button" data-model-action="test">${ico('unplug')}测试连接</button></div>
        <div id="model-provider-error" class="field-error" role="alert"></div><div class="model-save-row"><button class="btn" type="button" data-model-action="reset">取消</button><button class="btn primary" type="submit">${ico('check')}保存配置</button></div>
      </form>
      <section class="model-catalog" aria-label="模型目录"><div class="model-catalog-heading"><div><h2>模型目录 <span class="muted">${p.models.length}</span></h2><p>模型 ID、名称与能力</p></div><div class="model-catalog-actions"><button class="btn quiet" type="button" data-model-action="fetch">${ico('refresh-cw')}获取模型</button><button class="btn" type="button" data-model-action="add-model">${ico('plus')}添加模型</button></div></div>
      <div class="model-table"><div class="model-table-head"><span>模型</span><span>能力</span><span>启用</span><span>操作</span></div>${p.models.length ? p.models.map(m => `<div class="model-table-row"><div class="model-cell-name"><strong>${esc(m.name || m.modelId)}</strong><span title="${esc(m.modelId)}">${esc(m.modelId)}</span></div><span class="tag ${m.type === 'vision' ? 'doing' : m.type === 'embedding' ? 'priority-high' : ''}">${types[m.type] || '未设置'}</span><input type="checkbox" aria-label="启用 ${esc(m.name || m.modelId)}" data-model-toggle="${esc(m.id)}" ${m.enabled ? 'checked' : ''}><div>${btn('edit-model','pencil','编辑模型',`data-id="${esc(m.id)}"`)}${btn('remove-model','trash-2','移除模型',`data-id="${esc(m.id)}"`)}</div></div>`).join('') : '<div class="empty">暂无模型</div>'}</div></section>`;
  }

  function providerModal() {
    openOverlay(`<section class="modal" role="dialog" aria-modal="true" aria-labelledby="model-new-title"><h2 id="model-new-title">添加供应商</h2><form id="model-new-provider-form"><label for="model-template">供应商类型</label><select class="field-input" id="model-template" name="template"><option value="custom">自定义供应商</option><option value="deepseek">DeepSeek</option><option value="local">本地模型</option></select><label for="model-new-name">显示名称</label><input class="field-input" id="model-new-name" name="name" required maxlength="60" placeholder="例如：团队模型服务"><div id="model-new-error" class="field-error" role="alert"></div><div class="modal-actions"><button class="btn" type="button" data-model-action="close">取消</button><button class="btn primary" type="submit">添加供应商</button></div></form></section>`, true);
  }

  function modelModal(id) {
    const p = selected(), m = p.models.find(item => item.id === id);
    openOverlay(`<section class="modal" role="dialog" aria-modal="true" aria-labelledby="model-edit-title"><h2 id="model-edit-title">${m ? '编辑' : '添加'}模型</h2><form id="model-edit-form" data-id="${esc(m?.id || '')}" data-provider="${esc(p.id)}"><label for="model-id-input">模型 ID</label><input id="model-id-input" class="field-input" name="modelId" required maxlength="180" value="${esc(m?.modelId || '')}" placeholder="服务商提供的模型标识"><label for="model-display-name">显示名称</label><input id="model-display-name" class="field-input" name="name" maxlength="60" value="${esc(m?.name || '')}" placeholder="可选"><label for="model-type">能力类型</label><select id="model-type" class="field-input" name="type">${typeOptions(m?.type || 'chat')}</select><label class="model-modal-checkbox"><input type="checkbox" name="enabled" ${!m || m.enabled ? 'checked' : ''}> 启用模型</label><div id="model-edit-error" class="field-error" role="alert"></div><div class="modal-actions"><button type="button" class="btn" data-model-action="close">取消</button><button class="btn primary" type="submit">保存模型</button></div></form></section>`, true);
  }

  function removeConfirmation(kind, item) {
    const p = selected(), users = allRefs(p.id, kind === 'model' ? item.id : undefined);
    if (users.length) return showToast(`无法移除，以下配置正在使用：${users.join('、')}`);
    openOverlay(`<section class="modal" role="dialog" aria-modal="true" aria-labelledby="model-remove-title"><h2 id="model-remove-title">移除${kind === 'model' ? '模型' : '供应商'}</h2><p>确认移除「${esc(item.name || item.modelId)}」${kind === 'provider' ? '及其模型目录' : ''}？</p><div class="modal-actions"><button type="button" class="btn" data-model-action="close">取消</button><button type="button" class="btn danger" data-model-action="confirm-remove" data-kind="${kind}" data-id="${esc(item.id)}" data-provider="${esc(p.id)}">确认移除</button></div></section>`, true);
  }

  document.addEventListener('click', event => {
    const target = event.target.closest('[data-model-action]');
    if (!target) return;
    const action = target.dataset.modelAction, p = selected();
    if (action === 'select') { selectedId = target.dataset.id; render(); }
    if (action === 'add-provider') providerModal();
    if (action === 'add-model') modelModal();
    if (action === 'edit-model') modelModal(target.dataset.id);
    if (action === 'close') closeOverlay();
    if (action === 'reset') render();
    if (action === 'clear-key') document.getElementById('model-key').value = '';
    if (action === 'test' || action === 'fetch') { const key = document.getElementById('model-key'); if (key) key.value = ''; showToast('设计预览尚未连接模型服务，未发送请求'); }
    if (action === 'remove-provider' && p) removeConfirmation('provider', p);
    if (action === 'remove-model') { const m = p?.models.find(m => m.id === target.dataset.id); if (m) removeConfirmation('model', m); }
    if (action === 'confirm-remove') {
      const provider = providers().find(item => item.id === target.dataset.provider);
      if (!provider) return closeOverlay();
      const users = allRefs(provider.id, target.dataset.kind === 'model' ? target.dataset.id : undefined);
      if (users.length) return showToast(`无法移除，以下配置正在使用：${users.join('、')}`);
      if (target.dataset.kind === 'model') provider.models = provider.models.filter(m => m.id !== target.dataset.id);
      else window.aiStore.providers = providers().filter(item => item.id !== provider.id);
      closeOverlay(); render(); showToast('已移除');
    }
  });

  document.addEventListener('change', event => {
    const target = event.target;
    if (target.matches('[data-model-toggle]')) { const m = selected()?.models.find(item => item.id === target.dataset.modelToggle); if (m) { m.enabled = target.checked; showToast(m.enabled ? '模型已启用' : '模型已停用'); } }
    if (target.id === 'model-template') document.getElementById('model-new-name').value = ({ deepseek: 'DeepSeek', local: '本地模型', custom: '' })[target.value];
  });

  document.addEventListener('submit', event => {
    const form = event.target;
    if (!['model-provider-form', 'model-new-provider-form', 'model-edit-form'].includes(form.id)) return;
    event.preventDefault();
    // 密钥不参与 FormData，且在任何保存结果之前清空输入框。
    const key = document.getElementById('model-key'); if (key) key.value = '';
    const data = new FormData(form);
    if (form.id === 'model-provider-form') {
      const p = providers().find(item => item.id === form.dataset.id), name = String(data.get('name')).trim(), baseUrl = String(data.get('baseUrl')).trim();
      let validUrl = false;
      try { const url = new URL(baseUrl); validUrl = ['https:', 'http:'].includes(url.protocol) && !url.username && !url.password; } catch {}
      if (!name || !validUrl) { document.getElementById('model-provider-error').textContent = '请输入名称与有效的 HTTP 或 HTTPS 地址，地址不能包含账号或密码。'; return; }
      if (knowledgeRefs(p.id).length && (p.baseUrl!==baseUrl||p.protocol!==data.get('protocol'))) { document.getElementById('model-provider-error').textContent='此供应商被知识库引用，请先调整知识库的向量模型，再更改地址或协议。'; return; }
      Object.assign(p, { name, baseUrl, protocol: data.get('protocol'), enabled: data.has('enabled') });
      render(); showToast('配置已保存到本次预览，连接仍未验证');
    } else if (form.id === 'model-new-provider-form') {
      const name = String(data.get('name')).trim(), template = data.get('template');
      if (!name) { document.getElementById('model-new-error').textContent = '请输入供应商名称'; return; }
      selectedId = `provider-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
      providers().push({ id: selectedId, name, kind: template === 'deepseek' ? 'preset' : 'custom', baseUrl: template === 'deepseek' ? 'https://api.deepseek.com' : template === 'local' ? 'http://localhost:8000/v1' : '', protocol: 'openai-completions', enabled: true, models: [] });
      closeOverlay(); render(); showToast('供应商已添加');
    } else {
      const p = providers().find(item => item.id === form.dataset.provider), modelId = String(data.get('modelId')).trim(), type = data.get('type');
      if (!p) return closeOverlay();
      const users = refs(p.id, form.dataset.id), existing = p.models.find(m => m.id === form.dataset.id);
      let error = '';
      if (!modelId) error = '请输入模型 ID';
      else if (p.models.some(m => m.id !== form.dataset.id && m.modelId === modelId)) error = '此供应商下已有相同的模型 ID';
      else if (existing && type === 'embedding' && users.length) error = `以下 Agent 正在使用此模型，无法改为向量模型：${users.map(a => a.name).join('、')}`;
      else if (existing && knowledgeRefs(p.id,existing.id).length && (type!=='embedding'||modelId!==existing.modelId)) error='此向量模型被知识库引用，暂不能改变模型 ID 或能力类型。';
      if (error) { document.getElementById('model-edit-error').textContent = error; return; }
      const values = { modelId, name: String(data.get('name')).trim() || modelId, type, enabled: data.has('enabled') };
      if (existing) Object.assign(existing, values);
      else p.models.push({ id: `model-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`, ...values });
      closeOverlay(); render(); showToast('模型已保存');
    }
  });

  window.modelPages = { render: renderPage };
})();
