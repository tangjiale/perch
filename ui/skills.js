(() => {
  let filter = 'all';
  let pendingImport = null;
  let importGeneration = 0;
  const skills = () => window.resourceStore.skills;
  const find = id => skills().find(skill => skill.id === id);
  const references = id => (window.aiStore?.agents || []).filter(agent => (agent.skillIds || []).includes(id));
  const button = (action, id, label, icon, style = 'icon-btn') => `<button type="button" class="${style}" data-skill-action="${action}" data-id="${esc(id || '')}" title="${esc(label)}" aria-label="${esc(label)}">${ico(icon)}${style.includes('btn ') ? esc(label) : ''}</button>`;

  function validate(value, id = '') {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return '技能必须是一个包含名称和指令正文的对象。';
    if (typeof value.name !== 'string' || !value.name.trim() || value.name.trim().length > 60) return '名称不能为空，且最多 60 个字符。';
    if (typeof value.description !== 'string' || value.description.length > 500) return '描述必须为文字，且最多 500 个字符。';
    if (typeof value.content !== 'string' || !value.content.trim() || value.content.length > 60000) return '指令正文不能为空，且最多 60,000 个字符。';
    if (skills().some(skill => skill.id !== id && skill.name.trim().toLowerCase() === value.name.trim().toLowerCase())) return '已有同名技能，请修改名称后重试。';
    return '';
  }

  function renderPage() {
    const query = String(window.searchTerm || '').trim().toLowerCase();
    const items = skills().filter(skill => (filter === 'all' || (filter === 'enabled' ? skill.enabled : !skill.enabled)) && `${skill.name} ${skill.description}`.toLowerCase().includes(query));
    return `<section class="skills-page"><div class="page-heading"><div><h1>技能</h1><p>${skills().length} 个技能 · ${skills().filter(skill => skill.enabled).length} 个已启用</p></div><div class="skill-heading-actions">${button('import', '', '导入技能', 'upload', 'btn secondary')}${button('create', '', '新建技能', 'plus', 'btn primary')}</div></div>
      <div class="skill-toolbar"><div class="section-tabs" role="group" aria-label="技能状态">${[['all','全部技能'],['enabled','已启用'],['disabled','已停用']].map(([value,label]) => `<button data-skill-action="filter" data-filter="${value}" class="${filter === value ? 'active' : ''}" aria-pressed="${filter === value}">${label}</button>`).join('')}</div></div>
      <div class="skill-list" role="table" aria-label="技能列表"><div class="skill-list-head" role="row"><span role="columnheader">名称 / 描述</span><span role="columnheader">来源</span><span role="columnheader">关联 Agent</span><span role="columnheader">状态</span><span role="columnheader">操作</span></div>${items.map(skill => `<div class="skill-list-row" role="row"><div class="skill-identity" role="cell"><span class="skill-symbol">${ico('file-code-2')}</span><div><button class="skill-name" data-skill-action="edit" data-id="${esc(skill.id)}">${esc(skill.name)}</button><p title="${esc(skill.description)}">${esc(skill.description || '暂无描述')}</p></div></div><div class="skill-origin" role="cell">${ico(skill.source === 'imported' ? 'download' : 'file-pen-line')}${skill.source === 'imported' ? '文件导入' : '本地创建'}</div><div class="skill-references" role="cell">${references(skill.id).length} 个 Agent</div><div role="cell"><label class="skill-toggle"><input type="checkbox" data-skill-toggle="${esc(skill.id)}" ${skill.enabled ? 'checked' : ''} aria-label="启用 ${esc(skill.name)}"><span>${skill.enabled ? '已启用' : '已停用'}</span></label></div><div class="skill-row-actions" role="cell">${button('edit',skill.id,'编辑技能','square-pen')}${button('delete',skill.id,'删除技能','trash-2')}</div></div>`).join('') || '<div class="empty">没有匹配的技能</div>'}</div><div class="skill-list-footer">显示 ${items.length} 个技能</div></section>`;
  }

  function openEditor(id) {
    const original = find(id);
    if (id && !original) return;
    const skill = original || {name:'',description:'',content:'',enabled:true};
    openOverlay(`<section class="drawer skill-drawer" role="dialog" aria-modal="true" aria-labelledby="skill-editor-title"><div class="drawer-top"><span id="skill-editor-title">${original ? '编辑技能' : '新建技能'}</span>${button('close','','关闭技能配置','x')}</div><form id="skill-form" class="skill-form" data-id="${esc(original?.id || '')}"><div class="drawer-content"><div class="skill-editor-identity"><span class="skill-symbol">${ico('file-code-2')}</span><h2>${original ? esc(original.name) : '新技能'}</h2></div><div class="skill-field"><label for="skill-name">名称 <span>*</span></label><input class="field-input" id="skill-name" name="name" required maxlength="60" value="${esc(skill.name)}" placeholder="例如：会议纪要整理"></div><div class="skill-field"><label for="skill-description">描述</label><textarea class="field-input" id="skill-description" name="description" maxlength="500" rows="3" placeholder="适用场景与产出要求">${esc(skill.description)}</textarea></div><div class="skill-field"><label for="skill-content">技能指令 <span>*</span></label><textarea class="field-input skill-content" id="skill-content" name="content" required maxlength="60000" rows="14" placeholder="编写执行步骤、约束与输出格式">${esc(skill.content)}</textarea></div><label class="skill-toggle"><input type="checkbox" name="enabled" ${skill.enabled ? 'checked' : ''}>启用技能</label>${original ? `<div class="skill-editor-meta">${original.source === 'imported' ? '文件导入' : '本地创建'} · ${references(original.id).length} 个 Agent 关联</div>` : ''}<div id="skill-form-error" class="field-error" role="alert"></div></div><div class="drawer-bottom">${original ? button('delete',original.id,'删除技能','trash-2') : '<span></span>'}<div class="skill-heading-actions"><button type="button" class="btn" data-skill-action="close">取消</button><button type="submit" class="btn primary">${ico('check')}${original ? '保存更改' : '创建技能'}</button></div></div></form></section>`);
  }

  function openImport() {
    pendingImport = null;
    importGeneration++;
    openOverlay(`<section class="drawer skill-drawer" role="dialog" aria-modal="true" aria-labelledby="skill-import-title"><div class="drawer-top"><span id="skill-import-title">导入技能</span>${button('close','','关闭技能导入','x')}</div><div class="drawer-content"><h2>选择技能文件</h2><label for="skill-import-file" class="skill-file-label">Markdown / SKILL.md / JSON · 最大 256 KB</label><input id="skill-import-file" class="field-input" type="file" accept=".md,.json" aria-label="技能文件"><div id="skill-import-error" class="field-error" role="alert"></div><div id="skill-import-preview" aria-live="polite"></div></div><div class="drawer-bottom"><span class="muted">待确认导入</span><div class="skill-heading-actions"><button class="btn" data-skill-action="close">取消</button><button class="btn primary" data-skill-action="confirm-import" id="skill-import-confirm" disabled>${ico('check')}确认导入</button></div></div></section>`);
  }

  function parseImport(text, filename) {
    text = text.replace(/^\uFEFF/, '');
    if (!text.trim()) throw new Error('文件为空，请选择包含技能内容的文件。');
    let value;
    if (/\.json$/i.test(filename)) {
      try { value = JSON.parse(text); } catch { throw new Error('JSON 格式错误，请检查文件后重试。'); }
      if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('JSON 必须包含单个技能对象。');
      value = {name:value.name,description:value.description === undefined ? '' : value.description,content:value.content};
    } else if (/\.md$/i.test(filename)) {
      value = {name:filename.replace(/\.md$/i,''),description:'',content:text};
      if (/^---[ \t]*\r?\n/.test(text)) {
        const match = text.match(/^---[ \t]*\r?\n([\s\S]*?)\r?\n---[ \t]*(?:\r?\n|$)/);
        if (!match) throw new Error('YAML 元信息缺少结束分隔行 ---。');
        if (!window.jsyaml) throw new Error('YAML 解析器未加载，请刷新页面后重试。');
        let metadata;
        try { metadata = window.jsyaml.load(match[1], {schema:window.jsyaml.JSON_SCHEMA}); } catch { throw new Error('YAML 元信息格式错误，请检查名称与描述字段。'); }
        if (!metadata || typeof metadata !== 'object' || Array.isArray(metadata)) throw new Error('YAML 元信息必须是包含字段的对象。');
        value = {name:metadata.name === undefined ? value.name : metadata.name,description:metadata.description === undefined ? '' : metadata.description,content:text.slice(match[0].length)};
      }
    } else throw new Error('暂只支持 .md 和 .json 技能文件。');
    const error = validate(value);
    if (error) throw new Error(error);
    return {name:value.name.trim(),description:value.description.trim(),content:value.content.trim()};
  }

  async function readImport(input) {
    const generation = ++importGeneration;
    pendingImport = null;
    const preview = document.getElementById('skill-import-preview');
    const errorNode = document.getElementById('skill-import-error');
    const confirm = document.getElementById('skill-import-confirm');
    confirm.disabled = true; preview.innerHTML = ''; errorNode.textContent = '';
    const file = input.files?.[0];
    if (!file) return;
    try {
      if (!/\.(md|json)$/i.test(file.name)) throw new Error('暂只支持 .md 和 .json 技能文件。');
      if (!file.size) throw new Error('文件为空，请选择包含技能内容的文件。');
      if (file.size > 256 * 1024) throw new Error('文件超过 256 KB，请缩减内容后重试。');
      const text = await file.text();
      if (generation !== importGeneration || !input.isConnected) return;
      pendingImport = parseImport(text, file.name);
      preview.innerHTML = `<div class="skill-import-meta"><span>名称</span><strong>${esc(pendingImport.name)}</strong><span>描述</span><p>${esc(pendingImport.description || '暂无描述')}</p></div><h3>技能指令</h3><pre class="skill-import-content">${esc(pendingImport.content)}</pre>`;
      confirm.disabled = false;
    } catch (error) {
      if (generation === importGeneration && input.isConnected) errorNode.textContent = error.message || '文件无法读取，请重新选择。';
    }
  }

  function confirmDelete(id) {
    const skill = find(id);
    if (!skill) return;
    if (references(id).length) { showToast('技能仍被 Agent 关联，请先在 Agent 配置中移除关联'); return; }
    openOverlay(`<section class="modal" role="dialog" aria-modal="true" aria-labelledby="skill-delete-title"><h2 id="skill-delete-title">删除技能</h2><p>确认删除「${esc(skill.name)}」？此操作无法撤销。</p><div class="modal-actions"><button class="btn" data-skill-action="edit" data-id="${esc(id)}">取消</button><button class="btn danger" data-skill-action="confirm-delete" data-id="${esc(id)}">${ico('trash-2')}删除</button></div></section>`,true);
  }

  document.addEventListener('click', event => {
    const target = event.target.closest('[data-skill-action]');
    if (!target || target.disabled) return;
    const action = target.dataset.skillAction, id = target.dataset.id;
    if (action === 'close') { pendingImport = null; importGeneration++; closeOverlay(); }
    if (action === 'create' || action === 'edit') openEditor(id);
    if (action === 'filter') { filter = target.dataset.filter; render(); }
    if (action === 'import') openImport();
    if (action === 'delete') confirmDelete(id);
    if (action === 'confirm-delete' && find(id)) {
      if (references(id).length) { showToast('技能仍被 Agent 关联，无法删除'); return; }
      window.resourceStore.skills = skills().filter(skill => skill.id !== id);
      closeOverlay(); render(); showToast('技能已删除');
    }
    if (action === 'confirm-import' && pendingImport) {
      const error = validate(pendingImport);
      if (error) { document.getElementById('skill-import-error').textContent = error; return; }
      skills().push({id:`skill-${crypto.randomUUID()}`,...pendingImport,source:'imported',enabled:true});
      pendingImport = null; filter = 'all'; closeOverlay(); render(); showToast('技能已导入');
    }
  });
  document.addEventListener('change', event => {
    if (event.target.id === 'skill-import-file') readImport(event.target);
    if (event.target.matches('[data-skill-toggle]')) {
      const skill = find(event.target.dataset.skillToggle);
      if (skill) { skill.enabled = event.target.checked; render(); showToast(skill.enabled ? '技能已启用' : '技能已停用'); }
    }
  });
  document.addEventListener('submit', event => {
    if (event.target.id !== 'skill-form') return;
    event.preventDefault();
    const data = new FormData(event.target), id = event.target.dataset.id;
    const value = {name:String(data.get('name') || '').trim(),description:String(data.get('description') || '').trim(),content:String(data.get('content') || '').trim(),enabled:data.get('enabled') === 'on'};
    const error = validate(value, id);
    if (error) { document.getElementById('skill-form-error').textContent = error; return; }
    const original = find(id);
    if (id && !original) return;
    if (original) Object.assign(original, value);
    else skills().push({id:`skill-${crypto.randomUUID()}`,...value,source:'local'});
    filter = 'all'; closeOverlay(); render(); showToast(original ? '技能已更新' : '技能已创建');
  });
  window.skillPages = {render:renderPage,openEditor};
})();
