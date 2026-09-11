(() => {
  let filter = 'all';
  let query = '';
  const agentIcons = ['list-checks', 'code-2', 'file-pen-line', 'scan-eye', 'bot', 'briefcase-business', 'notebook-pen', 'lightbulb'];
  const store = () => window.aiStore;
  const availableModels = provider => provider?.enabled ? provider.models.filter(model => model.enabled && ['chat', 'vision'].includes(model.type)) : [];
  function resolveModel(agent) {
    const provider = store().providers.find(item => item.id === agent.providerId);
    const model = provider?.models.find(item => item.id === agent.modelId);
    return {provider, model, available: !!(provider?.enabled && model?.enabled && ['chat', 'vision'].includes(model.type))};
  }
  function card(agent) {
    const {provider, model, available} = resolveModel(agent);
    return `<article class="agent-card">
      <div class="agent-card-top"><span class="agent-symbol">${ico(agentIcons.includes(agent.icon) ? agent.icon : 'bot')}</span><div class="agent-card-title"><h2>${esc(agent.name)}</h2><span class="agent-state ${agent.enabled ? 'is-enabled' : ''}"><span class="dot ${agent.enabled ? 'green' : ''}"></span>${agent.enabled ? '已启用' : '已停用'}</span></div><button class="icon-btn" data-agent-action="edit" data-id="${esc(agent.id)}" aria-label="编辑 ${esc(agent.name)}" title="编辑 Agent">${ico('square-pen')}</button></div>
      <p class="agent-description" title="${esc(agent.description)}">${esc(agent.description || '暂无描述')}</p>
      <div class="agent-model-line ${available ? '' : 'agent-model-error'}">${ico('cpu')}<span title="${esc(model?.modelId || '')}">${esc(model?.name || model?.modelId || '模型已移除')}</span></div>
      <div class="agent-provider-line">${esc(provider?.name || '供应商已移除')}<span>${available ? `温度 ${esc(agent.temperature)} · ${esc(agent.maxTokens)} tokens` : '模型不可用，请更新配置'}</span></div>
      <div class="agent-card-actions"><button class="icon-btn" data-agent-action="copy" data-id="${esc(agent.id)}" aria-label="复制 ${esc(agent.name)}" title="复制 Agent">${ico('copy')}</button><button class="icon-btn" data-agent-action="toggle" data-id="${esc(agent.id)}" aria-label="${agent.enabled ? '停用' : '启用'} ${esc(agent.name)}" title="${agent.enabled ? '停用' : '启用'} Agent" ${!agent.enabled && !available ? 'disabled' : ''}>${ico(agent.enabled ? 'pause' : 'play')}</button><button class="btn agent-chat-btn" data-agent-action="chat" data-id="${esc(agent.id)}" ${!agent.enabled || !available ? 'disabled' : ''}>${ico('message-square')}开始聊天</button></div>
    </article>`;
  }
  function renderPage() {
    const agents = store().agents.filter(agent => (filter === 'all' || (filter === 'enabled' ? agent.enabled : !agent.enabled)) && `${agent.name} ${agent.description}`.toLowerCase().includes(query.toLowerCase()));
    return `<section class="agents-page"><div class="page-heading"><div><h1>Agent</h1><p>${store().agents.length} 个 Agent · ${store().agents.filter(agent => agent.enabled).length} 个已启用</p></div><button class="btn primary" data-agent-action="create">${ico('plus')}新建 Agent</button></div>
      <div class="agent-toolbar"><div class="section-tabs" role="group" aria-label="Agent 状态筛选">${[['all','全部'],['enabled','已启用'],['disabled','已停用']].map(([value,label])=>`<button data-agent-action="filter" data-filter="${value}" class="${filter===value?'active':''}" aria-pressed="${filter===value}">${label}</button>`).join('')}</div><label class="agent-search">${ico('search')}<input id="agent-search" aria-label="搜索 Agent" placeholder="搜索 Agent" value="${esc(query)}"></label></div>
      <div class="agent-grid">${agents.map(card).join('')}</div>${agents.length ? '' : `<div class="agent-empty">${ico('bot')}<h2>${query ? '没有匹配的 Agent' : '暂无 Agent'}</h2><p>${query ? '尝试其他名称或描述' : '当前筛选下没有 Agent'}</p><button class="btn" data-agent-action="${query || filter !== 'all' ? 'reset' : 'create'}">${query || filter !== 'all' ? '清除筛选' : '新建 Agent'}</button></div>`}</section>`;
  }
  function modelOptions(providerId, selected = '') {
    const provider = store().providers.find(item => item.id === providerId);
    const models = availableModels(provider);
    return `<option value="">${models.length ? '选择模型' : '没有可用的对话模型'}</option>${models.map(model=>`<option value="${esc(model.id)}" ${model.id===selected?'selected':''}>${esc(model.name || model.modelId)}${model.type==='vision'?' · 视觉':''}</option>`).join('')}`;
  }
  function openEditor(id) {
    const original = store().agents.find(agent => agent.id === id);
    const providers = store().providers.filter(provider => provider.enabled);
    const agent = original || {name:'',description:'',icon:'bot',providerId:providers[0]?.id || '',modelId:'',systemPrompt:'',temperature:0.5,maxTokens:4096,enabled:true};
    const unavailable = original && !resolveModel(original).available;
    openOverlay(`<section class="drawer agent-drawer" role="dialog" aria-modal="true" aria-labelledby="agent-editor-title"><div class="drawer-top"><span id="agent-editor-title">${original?'编辑':'新建'} Agent</span><button class="icon-btn" data-agent-action="close" aria-label="关闭 Agent 配置" title="关闭">${ico('x')}</button></div>
      <form id="agent-form" class="agent-form" data-id="${esc(original?.id || '')}"><div class="drawer-content"><div class="agent-editor-identity"><span class="agent-symbol" id="agent-icon-preview">${ico(agentIcons.includes(agent.icon)?agent.icon:'bot')}</span><h2>${original ? esc(original.name) : '新 Agent'}</h2></div>
        <div class="agent-form-field"><label for="agent-name">名称 <span class="agent-required">*</span></label><input id="agent-name" class="field-input" name="name" required maxlength="60" value="${esc(agent.name)}" placeholder="例如：工作规划助手"></div>
        <div class="agent-form-field"><label for="agent-description">描述</label><textarea id="agent-description" class="field-input" name="description" maxlength="240" rows="2" placeholder="这个 Agent 擅长什么">${esc(agent.description)}</textarea></div>
        <div class="agent-form-field"><label id="agent-icon-label">图标</label><div class="agent-icon-options" role="group" aria-labelledby="agent-icon-label">${agentIcons.map((icon,index)=>`<label class="agent-icon-option" title="${['工作规划','研发','文档','视觉','通用','工作','笔记','创意'][index]}"><input type="radio" name="icon" value="${icon}" ${agent.icon===icon?'checked':''} aria-label="${['工作规划','研发','文档','视觉','通用','工作','笔记','创意'][index]}"><span>${ico(icon)}</span></label>`).join('')}</div></div>
        <div class="agent-form-field"><label for="agent-prompt">系统提示词 <span class="agent-required">*</span></label><textarea id="agent-prompt" class="field-input agent-prompt" name="systemPrompt" required maxlength="12000" rows="5" placeholder="定义角色、目标与回答方式">${esc(agent.systemPrompt)}</textarea></div>
        <fieldset class="agent-skill-field"><legend>技能</legend><div class="agent-skill-options">${resourceStore.skills.filter(skill=>skill.enabled||(agent.skillIds||[]).includes(skill.id)).map(skill=>`<label><input type="checkbox" name="skillIds" value="${esc(skill.id)}" ${(agent.skillIds||[]).includes(skill.id)?'checked':''}><span><strong>${esc(skill.name)}${skill.enabled?'':' · 已停用'}</strong><small>${esc(skill.description)}</small></span></label>`).join('')||'<span class="muted">暂无已启用技能</span>'}</div><a class="text-btn" href="#skills" data-agent-action="close">管理技能 ${ico('arrow-up-right')}</a></fieldset>
        <div class="agent-form-section"><h3>模型配置</h3>${unavailable?'<p class="field-error">原模型已停用、被移除或不支持对话，请重新选择。</p>':''}<div class="agent-form-field"><label for="agent-provider">供应商</label><select id="agent-provider" class="field-input" name="providerId" required><option value="">选择供应商</option>${providers.map(provider=>`<option value="${esc(provider.id)}" ${provider.id===agent.providerId?'selected':''}>${esc(provider.name)}</option>`).join('')}</select></div><div class="agent-form-field"><label for="agent-model">模型</label><select id="agent-model" class="field-input" name="modelId" required>${modelOptions(agent.providerId,agent.modelId)}</select></div>
        <div class="agent-numeric-fields"><div class="agent-form-field"><label for="agent-temperature">Temperature</label><input id="agent-temperature" class="field-input" name="temperature" type="number" min="0" max="2" step="0.1" required value="${esc(agent.temperature)}"></div><div class="agent-form-field"><label for="agent-max-tokens">最大输出 tokens</label><input id="agent-max-tokens" class="field-input" name="maxTokens" type="number" min="1" max="32768" step="1" required value="${esc(agent.maxTokens)}"></div></div></div>
        <label class="agent-enabled-control"><input name="enabled" type="checkbox" ${agent.enabled?'checked':''}>启用 Agent</label><div id="agent-form-error" class="field-error" role="alert"></div></div>
      <div class="drawer-bottom">${original?`<button type="button" class="icon-btn agent-delete" data-agent-action="delete" data-id="${esc(original.id)}" title="删除 Agent" aria-label="删除 Agent">${ico('trash-2')}</button>`:'<span></span>'}<div class="agent-save-actions"><button type="button" class="btn" data-agent-action="close">取消</button><button type="submit" class="btn primary">${ico('check')}保存</button></div></div></form></section>`);
  }
  function confirmDelete(id) {
    const agent = store().agents.find(item => item.id === id);
    if (!agent) return;
    if (store().conversations.some(conversation => conversation.agentId === id)) {
      showToast('此 Agent 关联已有会话，请选择停用以保留历史');
      return;
    }
    openOverlay(`<section class="modal" role="dialog" aria-modal="true" aria-labelledby="agent-delete-title"><h2 id="agent-delete-title">删除 Agent</h2><p>确定删除「${esc(agent.name)}」？此操作无法撤销。</p><div class="modal-actions"><button class="btn" data-agent-action="edit" data-id="${esc(id)}">取消</button><button class="btn danger" data-agent-action="confirm-delete" data-id="${esc(id)}">${ico('trash-2')}删除</button></div></section>`,true);
  }
  document.addEventListener('click', event => {
    const target = event.target.closest('[data-agent-action]');
    if (!target || target.disabled) return;
    const action = target.dataset.agentAction;
    const id = target.dataset.id;
    const agent = store().agents.find(item => item.id === id);
    if (action === 'close') closeOverlay();
    if (action === 'create' || action === 'edit') openEditor(id);
    if (action === 'filter') {filter=target.dataset.filter;render();}
    if (action === 'reset') {filter='all';query='';render();}
    if (action === 'chat' && agent) {
      if (!agent.enabled || !resolveModel(agent).available) {showToast('请先启用 Agent 并配置可用模型');return;}
      window.chatPages.start(id);
    }
    if (action === 'copy' && agent) {
      store().agents.push({...agent,id:`agent-${crypto.randomUUID()}`,name:`${agent.name.slice(0,55)} 副本`,enabled:false});
      filter='all';query='';render();showToast('已复制 Agent，当前为停用状态');
    }
    if (action === 'toggle' && agent) {
      if (!agent.enabled && !resolveModel(agent).available) {showToast('模型不可用，请先更新配置');return;}
      agent.enabled=!agent.enabled;render();showToast(agent.enabled?'已启用 Agent':'已停用 Agent');
    }
    if (action === 'delete') confirmDelete(id);
    if (action === 'confirm-delete' && agent) {
      if (store().conversations.some(conversation=>conversation.agentId===id)) {showToast('Agent 关联已有会话，无法删除');return;}
      store().agents=store().agents.filter(item=>item.id!==id);closeOverlay();render();showToast('已删除 Agent');
    }
  });
  document.addEventListener('change', event => {
    if (event.target.id === 'agent-provider') document.getElementById('agent-model').innerHTML=modelOptions(event.target.value);
    if (event.target.closest('#agent-form') && event.target.name === 'icon') {
      document.getElementById('agent-icon-preview').innerHTML=ico(event.target.value);
      if (typeof icons === 'function') icons();
    }
  });
  document.addEventListener('input', event => {
    if (event.target.id !== 'agent-search') return;
    const cursor=event.target.selectionStart;
    query=event.target.value;render();
    const input=document.getElementById('agent-search');input?.focus();input?.setSelectionRange(cursor,cursor);
  });
  document.addEventListener('submit', event => {
    if (event.target.id !== 'agent-form') return;
    event.preventDefault();
    const form=event.target;
    const data=new FormData(form);
    const value={id:form.dataset.id || `agent-${crypto.randomUUID()}`,name:String(data.get('name')||'').trim(),description:String(data.get('description')||'').trim(),icon:String(data.get('icon')||'bot'),providerId:String(data.get('providerId')||''),modelId:String(data.get('modelId')||''),systemPrompt:String(data.get('systemPrompt')||'').trim(),temperature:Number(data.get('temperature')),maxTokens:Number(data.get('maxTokens')),enabled:data.get('enabled')==='on'};
    let error='';
    if (!value.name || !value.systemPrompt) error='名称和系统提示词不能为空或仅包含空格。';
    else if (!resolveModel(value).available) error='请选择已启用供应商下的可用对话或视觉模型。';
    else if (!String(data.get('temperature')||'').trim() || !Number.isFinite(value.temperature) || value.temperature<0 || value.temperature>2) error='Temperature 必须介于 0 与 2 之间。';
    else if (!String(data.get('maxTokens')||'').trim() || !Number.isInteger(value.maxTokens) || value.maxTokens<1 || value.maxTokens>32768) error='最大输出 tokens 必须是 1 至 32768 之间的整数。';
    if (error) {document.getElementById('agent-form-error').textContent=error;return;}
    const index=store().agents.findIndex(agent=>agent.id===value.id);
    value.skillIds=[...new Set(data.getAll('skillIds').map(String))].filter(id=>resourceStore.skills.some(skill=>skill.id===id));
    if (index<0) store().agents.push(value);else store().agents[index]=value;
    filter='all';query='';closeOverlay();render();showToast('Agent 已保存');
  });
  window.agentPages={render:renderPage,openEditor};
})();
