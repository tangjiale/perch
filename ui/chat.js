(() => {
  const store = window.aiStore;
  let currentId = store.conversations[0]?.id || null;
  const drafts = new Map();
  const pending = new Map();
  const uid = () => crypto.randomUUID();
  const current = () => store.conversations.find(c => c.id === currentId);
  const e = value => window.esc(value);
  const icon = name => window.ico(name);

  function snapshot(agent) {
    const provider = store.providers.find(p => p.id === agent.providerId);
    const model = provider?.models.find(m => m.id === agent.modelId);
    const skillsSnapshot=(agent.skillIds||[]).map(id=>resourceStore.skills.find(skill=>skill.id===id)).filter(skill=>skill?.enabled).map(skill=>({id:skill.id,name:skill.name,content:skill.content}));
    return {...structuredClone(agent),skillsSnapshot,providerName:provider?.name || '', modelName:model?.name || '', remoteModelId:model?.modelId || ''};
  }
  store.conversations.forEach(c => {
    const agent = store.agents.find(a => a.id === c.agentId);
    if (agent && !c.config) c.config = snapshot(agent);
  });
  function available(agent, config = agent) {
    const provider = store.providers.find(p => p.id === config?.providerId);
    const model = provider?.models.find(m => m.id === config?.modelId);
    return Boolean(agent?.enabled && provider?.enabled && model?.enabled && model.type !== 'embedding');
  }
  function refresh(scroll = false) {
    if (location.hash !== '#chat') return;
    render();
    if (scroll) requestAnimationFrame(() => {
      const messages = document.querySelector('.chat-messages');
      if (messages) messages.scrollTop = messages.scrollHeight;
    });
  }
  function start(agentId) {
    const agent = store.agents.find(a => a.id === agentId);
    if (!available(agent)) { showToast('请选择已启用的 Agent 和对话模型'); return; }
    const conversation = {id:uid(), agentId, config:snapshot(agent), knowledgeBaseId:null, title:'新对话', messages:[], createdAt:new Date().toISOString()};
    store.conversations.unshift(conversation);
    currentId = conversation.id;
    closeOverlay();
    if (location.hash === '#chat') refresh(true); else location.hash = 'chat';
  }
  function page() {
    const conversation = current();
    const agent = store.agents.find(a => a.id === conversation?.agentId);
    const config = conversation?.config || (agent ? snapshot(agent) : null);
    const modelEnabled = available(agent, config);
    const knowledge = resourceStore.findKnowledge(conversation?.knowledgeBaseId);
    const knowledgeAvailable = !conversation?.knowledgeBaseId || resourceStore.resolveEmbedding(knowledge).available;
    const enabled = modelEnabled && knowledgeAvailable;
    const busy = pending.has(currentId);
    const query = (window.searchTerm || '').toLowerCase();
    const history = store.conversations.filter(c => `${c.title} ${c.config?.name || ''} ${c.messages.map(m=>m.content).join(' ')}`.toLowerCase().includes(query));
    const choices = store.agents.filter(a => available(a));
    const draft = drafts.get(currentId) || '';
    return `<div class="chat-layout"><aside class="chat-history" aria-label="历史会话">
      <div class="chat-history-heading"><h1>聊天</h1><button class="icon-btn" data-chat-action="new" title="新对话" aria-label="新对话">${icon('square-pen')}</button></div>
      <button class="btn chat-new" data-chat-action="new">${icon('plus')}新对话</button>
      <div class="chat-history-label">会话 <span>${store.conversations.length}</span></div>
      <div class="chat-history-list">${history.map(c => `<button class="chat-history-item ${c.id===currentId?'active':''}" data-chat-action="select" data-id="${e(c.id)}"><span class="chat-history-icon">${icon(c.config?.icon || 'bot')}</span><span><strong>${e(c.title)}</strong><small>${e(c.config?.name || 'Agent 不可用')}</small></span></button>`).join('') || '<p class="chat-history-empty">暂无会话</p>'}</div>
      <a class="chat-manage-link" href="#agents">${icon('bot')}管理 Agent ${icon('arrow-up-right')}</a>
    </aside><section class="chat-conversation" aria-label="聊天内容">
      <header class="chat-header"><div class="chat-agent-identity"><span class="chat-agent-icon">${icon(config?.icon || 'bot')}</span><div><label class="sr-only" for="chat-agent-select">选择 Agent 开始新对话</label><select id="chat-agent-select" aria-label="选择 Agent 开始新对话">${!agent||!available(agent)?`<option value="" selected>${e(config?.name || '选择 Agent')}</option>`:''}${choices.map(a=>`<option value="${e(a.id)}" ${a.id===agent?.id?'selected':''}>${e(a.name)}</option>`).join('')}</select><p>${config?`${e(config.providerName)} <span>·</span> ${e(config.modelName)}`:'尚未选择模型'}</p></div></div><div class="chat-header-actions"><span class="chat-demo-status"><span class="dot amber"></span>演示对话</span><button class="icon-btn" data-chat-action="rename" title="重命名会话" aria-label="重命名会话" ${conversation?'':'disabled'}>${icon('pencil')}</button><button class="icon-btn" data-chat-action="delete" title="移除会话" aria-label="移除会话" ${conversation?'':'disabled'}>${icon('trash-2')}</button></div></header>
      <div class="chat-messages" role="log" aria-label="消息记录" aria-live="polite"><div class="chat-message-inner">${conversation?.messages.length?`<div class="chat-date-divider"><span>当前会话</span></div>${conversation.messages.map(m=>message(m,config)).join('')}`:`<div class="chat-empty"><span class="chat-empty-icon">${icon(config?.icon || 'messages-square')}</span><h2>${e(config?.name || '选择一位 Agent')}</h2><p>${e(config?.description || '暂无可用助手')}</p></div>`}${busy?`<article class="chat-message assistant"><span class="chat-message-avatar">${icon(config?.icon||'bot')}</span><div class="chat-message-body"><div class="chat-message-name">${e(config?.name)}<span class="chat-message-demo">演示</span></div><p class="chat-wait">${icon('loader-circle')}正在生成演示回复</p></div></article>`:''}</div></div>
      <div class="chat-compose-area">${!modelEnabled?`<div class="chat-unavailable">${icon('circle-alert')}当前 Agent 或模型不可用。<a href="#agents">检查 Agent 配置</a></div>`:''}${!knowledgeAvailable?`<div class="chat-unavailable">${icon('circle-alert')}知识库或向量模型不可用，请重新选择。<a href="#knowledge">管理知识库</a></div>`:''}
      <div class="chat-knowledge-control"><label for="chat-knowledge-select">${icon('library')}知识库</label><select id="chat-knowledge-select" ${!conversation||busy?'disabled':''}><option value="">不使用知识库</option>${conversation?.knowledgeBaseId&&!knowledge?'<option selected disabled>知识库已移除</option>':''}${resourceStore.knowledgeBases.map(kb=>`<option value="${e(kb.id)}" ${kb.id===conversation?.knowledgeBaseId?'selected':''} ${resourceStore.resolveEmbedding(kb).available?'':'disabled'}>${e(kb.name)}${resourceStore.resolveEmbedding(kb).available?'':' · 模型不可用'}</option>`).join('')}</select><a class="icon-btn" href="#knowledge" title="管理知识库" aria-label="管理知识库">${icon('arrow-up-right')}</a>${knowledge?`<span class="chat-knowledge-state">${knowledge.documents.length} 个文件 · 未索引</span>`:''}${config?.skillsSnapshot?.length?`<span class="chat-skill-count" title="${e(config.skillsSnapshot.map(skill=>skill.name).join('、'))}">${icon('puzzle')}${config.skillsSnapshot.length} 项技能</span>`:''}</div>
      <form id="chat-form" class="chat-composer"><label class="sr-only" for="chat-input">消息</label><textarea id="chat-input" maxlength="12000" rows="3" placeholder="${enabled?'输入消息…':'请先选择可用配置'}" ${enabled&&!busy?'':'disabled'}>${e(draft)}</textarea><div class="chat-compose-toolbar"><span>${icon('cpu')}${e(config?.remoteModelId || '未选择模型')}</span>${busy?`<button class="btn" type="button" data-chat-action="stop">${icon('square')}停止</button>`:`<button class="chat-send" type="submit" aria-label="发送消息" title="发送消息" ${enabled&&draft.trim()?'':'disabled'}>${icon('arrow-up')}</button>`}</div></form><p class="chat-preview-note">${icon('info')}演示回复，未调用模型或知识库检索 · 对话仅保留在本次预览中</p></div>
    </section></div>`;
  }
  function message(m, config) {
    return `<article class="chat-message ${m.role}"><span class="chat-message-avatar ${m.role==='user'?'user-avatar':''}">${m.role==='user'?'我':icon(config?.icon || 'bot')}</span><div class="chat-message-body"><div class="chat-message-name">${m.role==='user'?'你':e(config?.name || 'Agent')} ${m.role==='assistant'?'<span class="chat-message-demo">演示</span>':''}</div>${m.knowledge?`<div class="chat-message-knowledge">${icon('library')}${e(m.knowledge.name)} · 未执行检索</div>`:''}<div class="chat-message-text">${e(m.content)}</div><button class="icon-btn chat-copy" data-chat-action="copy" data-id="${e(m.id)}" aria-label="复制消息" title="复制消息">${icon('copy')}</button></div></article>`;
  }
  function send() {
    const conversation = current();
    const agent = store.agents.find(a=>a.id===conversation?.agentId);
    if (!conversation || !available(agent, conversation.config) || pending.has(currentId)) return;
    const kb=resourceStore.findKnowledge(conversation.knowledgeBaseId);
    if (conversation.knowledgeBaseId&&!resourceStore.resolveEmbedding(kb).available) { showToast('知识库或向量模型不可用，请重新选择'); return; }
    const knowledge=kb?{id:kb.id,name:kb.name,providerId:kb.providerId,modelId:kb.modelId,remoteModelId:resourceStore.resolveEmbedding(kb).model.modelId,documentIds:kb.documents.map(doc=>doc.id)}:null;
    const input = document.getElementById('chat-input');
    const content = input.value.trim();
    if (!content) return;
    conversation.messages.push({id:uid(),role:'user',content,knowledge});
    if(conversation.title==='新对话')conversation.title=content.slice(0,24);
    drafts.set(conversation.id,'');
    const id = conversation.id;
    const timer = setTimeout(() => {
      pending.delete(id);
      if (!store.conversations.includes(conversation)) return;
      conversation.messages.push({id:uid(),role:'assistant',demo:true,knowledge,content:`这是「${conversation.config.name}」的演示回复，尚未调用 ${conversation.config.modelName}。${knowledge?`\n\n已选择知识库「${knowledge.name}」，其中 ${knowledge.documentIds.length} 个文件尚未解析或向量化，本次没有检索文档。`:''}${conversation.config.skillsSnapshot?.length?`\n\n当前会话配置了技能：${conversation.config.skillsSnapshot.map(skill=>skill.name).join('、')}。尚未执行技能指令。`:''}`});
      if(currentId===id)refresh(true);
    },1100);
    pending.set(id,timer);
    refresh(true);
  }
  function stop(id) {
    clearTimeout(pending.get(id));pending.delete(id);
    const conversation=store.conversations.find(c=>c.id===id);
    if(conversation)conversation.messages.push({id:uid(),role:'assistant',content:'已停止生成演示回复。',demo:true});
    refresh(true);
  }
  document.addEventListener('click',async event=>{
    const button=event.target.closest('[data-chat-action]');if(!button)return;
    const action=button.dataset.chatAction,conversation=current();
    if(action==='new'){const candidate=store.agents.find(a=>a.id===conversation?.agentId&&available(a))||store.agents.find(a=>available(a));if(candidate)start(candidate.id);else{showToast('请先配置可用的 Agent 与模型');location.hash='agents';}}
    if(action==='select'){currentId=button.dataset.id;refresh(true);}
    if(action==='stop')stop(currentId);
    if(action==='copy'){const msg=conversation?.messages.find(m=>m.id===button.dataset.id);if(msg)try{await navigator.clipboard.writeText(msg.content);showToast('消息已复制');}catch{showToast('当前浏览器不允许复制，请选择消息文本复制');}}
    if(action==='rename'&&conversation)openOverlay(`<section class="modal" role="dialog" aria-modal="true" aria-labelledby="chat-rename-title"><h2 id="chat-rename-title">重命名会话</h2><form id="chat-rename-form" data-id="${e(conversation.id)}"><label for="chat-title-input">会话名称</label><input class="field-input" id="chat-title-input" name="title" value="${e(conversation.title)}" maxlength="80" required><div class="modal-actions"><button class="btn" type="button" data-action="close">取消</button><button class="btn primary" type="submit">保存</button></div></form></section>`,true);
    if(action==='delete'&&conversation)openOverlay(`<section class="modal" role="dialog" aria-modal="true" aria-labelledby="chat-delete-title"><h2 id="chat-delete-title">移除这段会话？</h2><p>${e(conversation.title)}</p><div class="modal-actions"><button class="btn" data-action="close">取消</button><button class="btn danger" data-chat-action="confirm-delete" data-id="${e(conversation.id)}">移除会话</button></div></section>`,true);
    if(action==='confirm-delete'){
      const id=button.dataset.id,index=store.conversations.findIndex(c=>c.id===id);if(index<0)return;
      const [removed]=store.conversations.splice(index,1);clearTimeout(pending.get(id));pending.delete(id);drafts.delete(id);
      currentId=store.conversations[0]?.id||null;closeOverlay();refresh();showToast('会话已移除',()=>{store.conversations.splice(index,0,removed);currentId=removed.id;refresh(true);});
    }
  });
  document.addEventListener('change',event=>{
    if(event.target.id==='chat-agent-select'&&event.target.value)start(event.target.value);
    if(event.target.id==='chat-knowledge-select'){
      const conversation=current(),id=event.target.value,kb=resourceStore.findKnowledge(id);
      if(!conversation||pending.has(currentId))return;
      if(id&&(!kb||!resourceStore.resolveEmbedding(kb).available)){showToast('请选择可用的知识库');refresh();return;}
      conversation.knowledgeBaseId=id||null;
      refresh();
    }
  });
  document.addEventListener('input',event=>{if(event.target.id==='chat-input'){drafts.set(currentId,event.target.value);const sendButton=document.querySelector('.chat-send');if(sendButton)sendButton.disabled=!event.target.value.trim();}});
  document.addEventListener('keydown',event=>{if(event.target.id==='chat-input'&&event.key==='Enter'&&!event.shiftKey&&!event.isComposing&&event.keyCode!==229){event.preventDefault();send();}});
  document.addEventListener('submit',event=>{
    if(event.target.id==='chat-form'){event.preventDefault();send();}
    if(event.target.id==='chat-rename-form'){event.preventDefault();const conversation=store.conversations.find(c=>c.id===event.target.dataset.id),title=new FormData(event.target).get('title').trim();if(conversation&&title){conversation.title=title;closeOverlay();refresh();}}
  });
  window.chatPages={render:page,start};
})();
