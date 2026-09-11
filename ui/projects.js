(() => {
  let sourceFilter = 'all';
  const labels = {todo:'待做',doing:'正在做',done:'已完成',closed:'已关闭'};
  const projectTasks = id => window.projectTaskAPI.getTasks().filter(task => task.projectId === id);
  const source = project => `<span class="project-origin ${project.source}">${ico(project.source === 'zentao' ? 'waypoints' : 'folder')}${project.source === 'zentao' ? `禅道项目 #${esc(project.remoteId)}` : '本地项目'}</span>`;
  const status = value => `<span class="project-status ${esc(value)}"><span></span>${labels[value] || '未知状态'}</span>`;
  const counts = items => Object.keys(labels).map(key => ({key,count:items.filter(item => item.status === key).length}));
  const distribution = items => {
    const values = counts(items);
    const description = values.map(({key,count})=>`${labels[key]} ${count} 项`).join('，');
    return `<div class="project-distribution" role="img" aria-label="${description}" title="${description}">${values.filter(item=>item.count).map(({key,count})=>`<span class="project-distribution-segment ${key}" style="flex:${count}"></span>`).join('')}</div>`;
  };
  const action = (name,id,label,icon,classes='icon-btn') => `<button type="button" class="${classes}" data-project-action="${name}" data-project-id="${esc(id)}" title="${label}" aria-label="${label}">${ico(icon)}${classes.includes('btn ') ? label : ''}</button>`;

  function renderPage() {
    const all = window.projectStore.projects;
    const query = String(window.searchTerm || '').trim().toLowerCase();
    const visible = all.filter(project => (sourceFilter === 'all' || project.source === sourceFilter) && `${project.name} ${project.description} ${project.owner} ${project.remoteId || ''}`.toLowerCase().includes(query));
    return `<div class="projects-page"><div class="page-heading"><div><h1>项目</h1><p>${all.length} 个项目 · ${all.filter(project=>project.source==='local').length} 个本地 · ${all.filter(project=>project.source==='zentao').length} 个禅道</p></div><div class="project-heading-actions">${action('sync','','从禅道同步','refresh-cw','btn secondary')}${action('new','','新建项目','plus','btn primary')}</div></div>
      <div class="project-toolbar"><div class="section-tabs" role="group" aria-label="项目来源">${[['all','全部项目'],['local','本地项目'],['zentao','禅道项目']].map(([key,label])=>`<button class="${sourceFilter===key?'active':''}" data-project-action="filter" data-project-id="${key}" aria-pressed="${sourceFilter===key}">${label}<span class="muted">${key==='all'?all.length:all.filter(project=>project.source===key).length}</span></button>`).join('')}</div><span class="project-sync-state">${ico('circle-dashed')}禅道未连接</span></div>
      <div class="project-list" role="table" aria-label="项目列表"><div class="project-list-head" role="row"><span role="columnheader">项目名称 / 来源</span><span role="columnheader">项目状态</span><span role="columnheader">关联任务</span><span role="columnheader">负责人 / 截止日期</span><span role="columnheader">操作</span></div>${visible.map(project => {
        const items = projectTasks(project.id);
        return `<div class="project-list-row" role="row"><div class="project-identity" role="cell"><span class="project-symbol ${project.source}">${ico(project.source==='zentao'?'waypoints':'folder-kanban')}</span><div><button class="project-name" data-project-action="detail" data-project-id="${esc(project.id)}">${esc(project.name)}</button><div class="project-description" title="${esc(project.description)}">${esc(project.description || '暂无描述')}</div><div class="project-source-line">${source(project)}${project.source==='zentao'?'<span class="muted">示例 · 未同步</span>':''}</div></div></div><div role="cell">${status(project.status)}</div><div class="project-task-summary" role="cell"><button class="project-task-link" data-project-action="tasks" data-project-id="${esc(project.id)}">${items.length} 项任务 ${ico('arrow-up-right')}</button>${distribution(items)}</div><div class="project-schedule" role="cell"><span>${ico('user-round')}${esc(project.owner || '未指定')}</span><time>${project.due ? esc(project.due) : '无截止日期'}</time></div><div class="project-row-actions" role="cell">${project.source==='local'?action('edit',project.id,'编辑项目','square-pen'):action('detail',project.id,'查看禅道项目信息','info')}${action('tasks',project.id,'查看项目看板','arrow-up-right')}</div></div>`;
      }).join('') || '<div class="empty">没有匹配的项目</div>'}</div><div class="project-list-footer"><span>显示 ${visible.length} 个项目</span><div class="project-legend">${Object.entries(labels).map(([key,label])=>`<span class="project-count ${key}"><i></i>${label}</span>`).join('')}</div></div></div>`;
  }

  function openDetail(id) {
    const project = window.projectStore.find(id);
    if (!project) return;
    const items = projectTasks(id);
    openOverlay(`<section class="drawer project-drawer" role="dialog" aria-modal="true" aria-labelledby="project-detail-title"><div class="drawer-top"><span>项目详情</span>${action('close','','关闭项目详情','x')}</div><div class="drawer-content">${source(project)}<h2 id="project-detail-title">${esc(project.name)}</h2><p class="project-detail-description">${esc(project.description || '暂无描述')}</p><dl class="project-detail-meta"><div><dt>项目状态</dt><dd>${status(project.status)}</dd></div><div><dt>负责人</dt><dd>${esc(project.owner || '未指定')}</dd></div><div><dt>截止日期</dt><dd>${esc(project.due || '未设置')}</dd></div>${project.source==='zentao'?`<div><dt>禅道项目 ID</dt><dd>#${esc(project.remoteId)}</dd></div><div><dt>连接标识</dt><dd>${esc(project.connectionId)}</dd></div><div><dt>同步状态</dt><dd>示例 · 未同步</dd></div>`:''}</dl><div class="project-detail-tasks"><div class="section-heading"><h3>关联任务 <span class="muted">${items.length}</span></h3>${action('new-task',id,'新建任务','plus')}</div><div class="project-detail-counts">${counts(items).map(({key,count})=>`<span>${status(key)}<b>${count}</b></span>`).join('')}</div>${items.map(task=>`<button class="project-linked-task" data-project-action="task-detail" data-project-id="${esc(task.id)}"><span><strong>${esc(task.title)}</strong><small>${task.source==='zentao'?`禅道执行 #${esc(task.remoteExecutionId || task.id.replace(/^E-/,''))}`:'个人任务'}</small></span>${status(task.status)}</button>`).join('') || '<div class="empty">暂无关联任务</div>'}</div></div><div class="drawer-bottom">${project.source==='local'?action('edit',id,'编辑项目','square-pen','btn secondary'):'<span>禅道项目信息只读</span>'}${action('tasks',id,'查看项目看板','columns-3','btn primary')}</div></section>`);
  }

  function openForm(id) {
    const project = id ? window.projectStore.find(id) : null;
    if (id && (!project || project.source !== 'local')) return;
    openOverlay(`<section class="drawer project-drawer" role="dialog" aria-modal="true" aria-labelledby="project-editor-title"><div class="drawer-top"><span>${project?'编辑项目':'新建本地项目'}</span>${action('close','','关闭项目编辑','x')}</div><div class="drawer-content"><h2 id="project-editor-title">${project ? esc(project.name) : '新建项目'}</h2><form id="project-editor-form" class="project-editor" data-project-id="${esc(id || '')}"><label for="project-name">项目名称 <span aria-hidden="true">*</span></label><input id="project-name" class="field-input" name="name" value="${esc(project?.name || '')}" required maxlength="80" placeholder="输入项目名称"><label for="project-description">项目描述</label><textarea id="project-description" class="field-input" name="description" rows="4" maxlength="500" placeholder="项目目标与工作范围">${esc(project?.description || '')}</textarea><label for="project-status">项目状态</label><select id="project-status" class="field-input" name="status">${Object.entries(labels).map(([value,label])=>`<option value="${value}" ${value===(project?.status || 'todo')?'selected':''}>${label}</option>`).join('')}</select><label for="project-owner">负责人</label><input id="project-owner" class="field-input" name="owner" maxlength="60" value="${esc(project?.owner ?? '我')}"><label for="project-due">截止日期</label><input id="project-due" class="field-input" name="due" type="date" value="${esc(project?.due || '')}"><div id="project-form-error" class="field-error" role="alert"></div></form>${project?`<div class="project-delete-zone">${action('delete',id,'删除项目','trash-2','btn danger')}${projectTasks(id).length?'<p>有关联任务，暂不可删除。</p>':''}</div>`:''}</div><div class="drawer-bottom"><button type="button" class="btn" data-project-action="close">取消</button><button type="submit" class="btn primary" form="project-editor-form">${project?'保存更改':'创建项目'}</button></div></section>`);
  }

  function confirmDelete(id) {
    const project = window.projectStore.find(id);
    if (!project || project.source !== 'local') return;
    if (projectTasks(id).length) { showToast('项目仍有关联任务，请先调整任务所属项目'); return; }
    openOverlay(`<section class="modal" role="dialog" aria-modal="true" aria-labelledby="project-delete-title"><h2 id="project-delete-title">删除项目</h2><p>确认删除「${esc(project.name)}」？此项目没有关联任务。</p><div class="modal-actions"><button class="btn" data-project-action="edit" data-project-id="${esc(id)}">取消</button><button class="btn danger" data-project-action="confirm-delete" data-project-id="${esc(id)}">删除项目</button></div></section>`, true);
  }

  document.addEventListener('click', event => {
    const button = event.target.closest('[data-project-action]');
    if (!button) return;
    const {projectAction:command, projectId:id} = button.dataset;
    if (command==='filter') { sourceFilter=id; render(); }
    if (command==='close') closeOverlay();
    if (command==='new') openForm();
    if (command==='edit') openForm(id);
    if (command==='detail') openDetail(id);
    if (command==='tasks') { closeOverlay(); window.projectTaskAPI.openTasks(id); }
    if (command==='new-task') { closeOverlay(); window.projectTaskAPI.createTask(id); }
    if (command==='task-detail') window.projectTaskAPI.openTask(id);
    if (command==='delete') confirmDelete(id);
    if (command==='confirm-delete') {
      const project = window.projectStore.find(id);
      if (!project || project.source!=='local' || projectTasks(id).length) { showToast('项目仍有关联任务，无法删除'); return; }
      window.projectStore.projects = window.projectStore.projects.filter(item=>item.id!==id);
      closeOverlay(); render(); showToast('项目已删除');
    }
    if (command==='sync') openOverlay(`<section class="modal" role="dialog" aria-modal="true" aria-labelledby="project-sync-title"><h2 id="project-sync-title">从禅道同步项目</h2><div class="notice">禅道未连接</div><p>当前项目为示例数据，尚未执行同步。</p><div class="modal-actions"><button class="btn" data-project-action="close">取消</button><a class="btn primary" href="#settings" data-project-action="close">配置禅道连接</a></div></section>`,true);
  });
  document.addEventListener('submit', event => {
    if (event.target.id !== 'project-editor-form') return;
    event.preventDefault();
    const form = event.target, data = new FormData(form), name = String(data.get('name') || '').trim();
    if (!name) { document.getElementById('project-form-error').textContent='请输入项目名称'; return; }
    const id = form.dataset.projectId, project = id ? window.projectStore.find(id) : null;
    if (id && (!project || project.source!=='local')) return;
    const value = {name,description:String(data.get('description') || '').trim(),status:String(data.get('status')),owner:String(data.get('owner') || '').trim(),due:String(data.get('due') || '')};
    if (!(value.status in labels)) return;
    if (project) Object.assign(project,value);
    else window.projectStore.projects.push({id:`local-${crypto.randomUUID()}`,source:'local',...value});
    closeOverlay(); render(); showToast(project?'项目已更新':'项目已创建');
  });
  window.projectPages = {render:renderPage,openDetail};
})();
