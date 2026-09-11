(() => {
  const initialApps = [
    { id: 'zentao', name: '禅道', description: '跟进项目执行、需求与缺陷，管理研发工作。', url: 'https://www.zentao.net', logo: '', category: '开发工具', favorite: true },
    { id: 'gitlab', name: 'GitLab', description: '代码仓库、合并请求与团队代码评审。', url: 'https://gitlab.com', logo: 'assets/gitlab.svg', category: '开发工具', favorite: true },
    { id: 'confluence', name: 'Confluence', description: '沉淀项目文档、技术方案与团队知识。', url: 'https://www.atlassian.com/software/confluence', logo: 'assets/confluence.svg', category: '协作办公', favorite: true },
    { id: 'figma', name: 'Figma', description: '查看界面设计、交互原型与设计交付。', url: 'https://www.figma.com', logo: 'assets/figma.svg', category: '协作办公', favorite: true },
    { id: 'jenkins', name: 'Jenkins', description: '运行构建流水线，查看部署结果与日志。', url: 'https://www.jenkins.io', logo: 'assets/jenkins.svg', category: '开发工具', favorite: false },
    { id: 'grafana', name: 'Grafana', description: '查看服务指标、监控仪表盘与告警。', url: 'https://grafana.com', logo: 'assets/grafana.svg', category: '运维监控', favorite: true },
    { id: 'github', name: 'GitHub', description: '管理代码项目，参与开源协作与问题讨论。', url: 'https://github.com', logo: 'assets/github.svg', category: '开发工具', favorite: false },
    { id: 'notion', name: 'Notion', description: '整理工作笔记、项目资料与个人知识库。', url: 'https://www.notion.so', logo: 'assets/notion.svg', category: '协作办公', favorite: false },
    { id: 'docker', name: 'Docker Hub', description: '查找容器镜像，管理镜像版本与发布。', url: 'https://hub.docker.com', logo: 'assets/docker.svg', category: '开发工具', favorite: false },
    { id: 'feishu', name: '飞书', description: '团队沟通、在线文档与会议协作。', url: 'https://www.feishu.cn', logo: '', category: '协作办公', favorite: false },
    { id: 'sentry', name: 'Sentry', description: '追踪应用异常，定位错误与性能问题。', url: 'https://sentry.io', logo: '', category: '运维监控', favorite: false },
    { id: 'outlook', name: 'Outlook', description: '处理工作邮件、会议邀请与联系人。', url: 'https://outlook.office.com', logo: '', category: '协作办公', favorite: false },
  ];
  window.demoApps = window.demoApps || initialApps;

  const styles = `<style>
    .app-directory-toolbar{display:flex;align-items:center;justify-content:space-between;gap:16px;margin:26px 0 20px;flex-wrap:wrap}
    .app-directory-filters{display:flex;align-items:center;gap:6px;flex-wrap:wrap}
    .app-filter{border:0;background:transparent;color:var(--muted);padding:8px 12px;border-radius:6px;font-size:13px;cursor:pointer}
    .app-filter.active{background:var(--blue-bg);color:var(--primary);font-weight:600}
    .app-filter:hover{background:var(--subtle)}
    .app-directory-grid{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:16px}
    .directory-app{position:relative;display:flex;flex-direction:column;min-height:176px;padding:16px;border:1px solid var(--border);border-radius:8px;background:var(--surface);min-width:0;transition:border-color 150ms}
    .directory-app:hover{border-color:var(--primary)}
    .directory-app-top{display:flex;align-items:center;gap:12px;min-width:0;padding-right:25px}
    .directory-app-logo{width:40px;height:40px;border-radius:8px;display:flex;align-items:center;justify-content:center;background:var(--subtle);flex-shrink:0;color:var(--primary);font-size:20px;font-weight:600}
    .directory-app-logo img{width:27px;height:27px;object-fit:contain}
    .directory-app-title{font-size:15px;line-height:1.4;font-weight:600;color:var(--text);text-decoration:none;overflow-wrap:anywhere}
    .directory-app-url{margin-top:4px;font-size:12px;color:var(--muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:100%}
    .directory-app-details{min-width:0}
    .directory-app-description{margin:12px 0 0;font-size:12px;line-height:18px;min-height:36px;color:var(--muted);overflow-wrap:anywhere;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}
    .directory-app-star{position:absolute;right:14px;top:16px;color:var(--muted)}
    .directory-app-star.is-favorite{color:var(--primary)}
    .directory-app-star.is-favorite svg{fill:var(--blue-bg)}
    .directory-app-footer{display:flex;align-items:center;justify-content:space-between;gap:8px;margin-top:auto;padding-top:8px}
    .directory-app-category{font-size:11px;color:var(--muted);background:var(--subtle);padding:3px 7px;border-radius:4px}
    .directory-app-actions{display:flex;align-items:center;gap:4px}
    .directory-app-actions a{display:inline-flex;align-items:center;justify-content:center;color:var(--muted);text-decoration:none}
    .directory-empty{grid-column:1/-1;text-align:center;padding:72px 20px;color:var(--muted);border-top:1px solid var(--border)}
    .connection-layout{display:block}
    .connection-main{max-width:880px;min-width:0}
    .connection-section{margin-bottom:28px;scroll-margin-top:20px}
    .connection-section h2{font-size:16px;margin:0;line-height:1.5}
    .connection-section p{margin:6px 0 0;color:var(--muted);font-size:13px;line-height:1.7}
    .connection-title-row{display:flex;justify-content:space-between;align-items:center;gap:12px}
    .connection-version{display:flex;align-items:center;gap:8px;font-size:12px;color:var(--muted);margin-top:10px}
    .connection-form{margin-top:22px;display:grid;gap:18px}
    .connection-field{display:grid;gap:7px;min-width:0}
    .connection-field label{font-size:13px;font-weight:500;color:var(--text)}
    .connection-field input{height:40px;width:100%;border:1px solid var(--border);border-radius:6px;background:var(--surface);color:var(--text);padding:0 12px;font-size:13px;box-sizing:border-box}
    .connection-field input::placeholder{color:var(--muted)}
    .connection-field input:focus{outline:2px solid var(--primary);outline-offset:2px}
    .connection-field small{font-size:12px;color:var(--muted);line-height:1.6}
    .connection-form-grid{display:grid;grid-template-columns:1fr 1fr;gap:16px}
    .connection-actions{display:flex;align-items:center;gap:14px}
    .connection-scope{display:flex;gap:10px;align-items:flex-start;margin-top:16px;padding:14px 16px;background:var(--subtle);border-radius:6px;color:var(--muted);font-size:13px;line-height:1.7}
    .connection-scope svg{flex-shrink:0;margin-top:2px}
    .connection-scope strong{color:var(--text);font-weight:500}
    .connection-switch-row{display:flex;justify-content:space-between;align-items:center;gap:24px;margin-top:18px}
    .connection-switch-label{font-size:13px;font-weight:500}
    .connection-switch[type=checkbox]{appearance:none;width:34px;height:20px;border-radius:12px;background:var(--border);position:relative;flex-shrink:0;cursor:pointer;margin:0}
    .connection-switch:before{content:'';position:absolute;width:14px;height:14px;top:3px;left:3px;border-radius:50%;background:var(--surface);transition:transform 150ms}
    .connection-switch[type=checkbox]:checked{background:var(--primary)}
    .connection-switch:checked:before{transform:translateX(14px)}
    .connection-permissions{display:grid;grid-template-columns:1fr 1fr;gap:8px 20px;margin-top:18px}
    .connection-permission{display:flex;gap:8px;align-items:center;font-size:12px;color:var(--muted)}
    .connection-permission svg{flex-shrink:0}
    .connection-footnote{display:flex;gap:8px;align-items:flex-start;font-size:12px;line-height:1.7;color:var(--muted);margin-top:22px}
    .connection-footnote svg{flex-shrink:0;margin-top:2px}
    @media(max-width:1199px){.app-directory-grid{grid-template-columns:repeat(3,minmax(0,1fr))}}
    @media(max-width:1100px){.directory-app{padding:16px}.directory-app-url{font-size:11px}}
    @media(max-width:899px){.app-directory-grid{grid-template-columns:repeat(2,minmax(0,1fr))}}
    @media(max-width:559px){.app-directory-grid{grid-template-columns:1fr}.connection-form-grid,.connection-permissions{grid-template-columns:1fr}.connection-title-row{align-items:flex-start}}
    @media(prefers-reduced-motion:reduce){.directory-app,.connection-switch:before{transition:none}}
  </style>`;

  function apps() {
    const e = window.esc;
    const ico = window.ico;
    const category = window.appCategory || '全部';
    const query = (window.searchTerm || '').toLowerCase().trim();
    const filtered = window.demoApps.filter(app =>
      (category === '全部' || (category === '收藏' ? app.favorite : category === '未分类' ? !app.category : app.category === category)) &&
      `${app.name} ${app.url} ${app.category} ${app.description || ''}`.toLowerCase().includes(query)
    );
    const categories = ['全部', '收藏', ...window.appCategories, ...(window.demoApps.some(app=>!app.category)?['未分类']:[])];
    return `${styles}
      <div class="page-heading"><div><h1>应用</h1><p class="muted">常用工具，触手可及</p></div><button class="btn primary" data-action="new-app">${ico('plus')} 添加应用</button></div>
      <div class="app-directory-toolbar">
        <div class="app-directory-filters" aria-label="应用分类">${categories.map(item => `<button class="app-filter ${category === item ? 'active' : ''}" data-action="app-filter" data-value="${e(item)}" aria-pressed="${category === item}">${item === '收藏' ? `${ico('star', 13)} ` : ''}${e(item)}</button>`).join('')}</div>
        <span class="muted" style="font-size:12px">${filtered.length} 个应用</span>
      </div>
      <div class="app-directory-grid">${filtered.map(app => {
        const url = new URL(app.url);
        const displayUrl = `${url.hostname}${url.pathname === '/' ? '' : url.pathname}`;
        return `<article class="directory-app">
          <button class="icon-btn directory-app-star ${app.favorite ? 'is-favorite' : ''}" data-action="star-app" data-id="${e(app.id)}" title="${app.favorite ? '取消收藏' : '收藏'} ${e(app.name)}" aria-label="${app.favorite ? '取消收藏' : '收藏'} ${e(app.name)}" aria-pressed="${app.favorite}">${ico('star', 16)}</button>
          <div class="directory-app-top"><div class="directory-app-logo">${app.logo ? `<img src="${e(app.logo)}" alt="${e(app.name)} 标志">` : e(app.name.slice(0, 1))}</div><div class="directory-app-details"><a class="directory-app-title" href="${e(app.url)}" target="_blank" rel="noopener noreferrer">${e(app.name)}</a><div class="directory-app-url" title="${e(app.url)}">${e(displayUrl)}</div></div></div>
          <p class="directory-app-description" title="${e(app.description || '')}">${e(app.description || '暂无描述')}</p>
          <div class="directory-app-footer"><span class="directory-app-category">${e(app.category || '未分类')}</span><div class="directory-app-actions"><button class="icon-btn" data-action="edit-app" data-id="${e(app.id)}" aria-label="编辑 ${e(app.name)}" title="编辑 ${e(app.name)}">${ico('pencil', 15)}</button><a class="icon-btn" href="${e(app.url)}" target="_blank" rel="noopener noreferrer" aria-label="打开 ${e(app.name)}" title="打开 ${e(app.name)}">${ico('arrow-up-right', 17)}</a></div></div>
        </article>`;
      }).join('') || '<div class="directory-empty">没有找到应用</div>'}</div>`;
  }

  let settingsTab = 'auth';
  function positionTokenHelp() {
    const panel = document.getElementById('zentao-token-help');
    if (!panel || panel.hidden) return;
    const rect = panel.parentElement.getBoundingClientRect();
    const below = window.innerHeight - rect.bottom - 20, above = rect.top - 50;
    const openBelow = below >= 400 || below >= above;
    panel.style.top = openBelow ? 'calc(100% + 8px)' : 'auto';
    panel.style.bottom = openBelow ? 'auto' : 'calc(100% + 8px)';
    panel.style.maxHeight = `${Math.max(140, openBelow ? below : above)}px`;
  }
  window.addEventListener('resize', positionTokenHelp);
  const settingTabs = [['auth','plug','禅道连接'],['scope','list-filter','同步范围'],['write','shield-check','管理权限'],['models','cpu','大模型管理']];

  function settingsTabs(selected = settingsTab) {
    return `<div class="settings-tabs" role="tablist" aria-label="设置分类">${settingTabs.map(([id,icon,label])=>`<button type="button" role="tab" id="settings-tab-${id}" data-settings-tab="${id}" aria-controls="settings-panel" aria-selected="${selected===id}" tabindex="${selected===id?'0':'-1'}">${window.ico(icon)}${label}</button>`).join('')}</div>`;
  }

  function activateSettingsTab(id) {
    if (!settingTabs.some(tab=>tab[0]===id)) return;
    if (id === 'models') { navigate('models'); return; }
    settingsTab = id;
    if (location.hash !== '#settings') { navigate('settings'); return; }
    document.querySelectorAll('[data-settings-tab]').forEach(tab=>{
      const active = tab.dataset.settingsTab === id;
      tab.setAttribute('aria-selected', String(active));
      tab.tabIndex = active ? 0 : -1;
    });
    document.querySelectorAll('[data-settings-section]').forEach(section=>{section.hidden=section.dataset.settingsSection!==id;});
    document.getElementById('settings-panel').setAttribute('aria-labelledby', `settings-tab-${id}`);
  }

  document.addEventListener('click', event=>{
    const helpButton = event.target.closest('#zentao-token-help-button');
    const help = document.getElementById('zentao-token-help');
    if (helpButton && help) {
      help.hidden = !help.hidden;
      helpButton.setAttribute('aria-expanded', String(!help.hidden));
      positionTokenHelp();
    } else if (help && !help.hidden && (event.target.closest('[data-close-token-help]') || !help.contains(event.target))) {
      help.hidden = true;
      document.getElementById('zentao-token-help-button').setAttribute('aria-expanded', 'false');
      if (event.target.closest('[data-close-token-help]')) document.getElementById('zentao-token-help-button').focus();
    }
    const tab=event.target.closest('[data-settings-tab]');
    if (tab) activateSettingsTab(tab.dataset.settingsTab);
    else if (event.target.closest('a[href="#settings"]')) activateSettingsTab('auth');
  });
  document.addEventListener('keydown', event=>{
    const help = document.getElementById('zentao-token-help');
    if (event.key === 'Escape' && help && !help.hidden) {
      help.hidden = true;
      const trigger = document.getElementById('zentao-token-help-button');
      trigger.setAttribute('aria-expanded', 'false');
      trigger.focus();
      return;
    }
    const tab=event.target.closest('[data-settings-tab]');
    if (!tab || !['ArrowLeft','ArrowRight','Home','End'].includes(event.key)) return;
    event.preventDefault();
    const tabs=[...document.querySelectorAll('[data-settings-tab]')], index=tabs.indexOf(tab);
    const next=event.key==='Home'?0:event.key==='End'?tabs.length-1:(index+(event.key==='ArrowRight'?1:-1)+tabs.length)%tabs.length;
    tabs[next].focus();
  });

  function settings() {
    const ico = window.ico;
    return `${styles}
      <div class="page-heading"><div><h1>设置</h1><p class="muted">连接工作系统，保留自己的工作节奏</p></div></div>
      ${settingsTabs()}
      <div class="connection-layout">
        <div class="connection-main" id="settings-panel" role="tabpanel" aria-labelledby="settings-tab-${settingsTab}" tabindex="0">
          <section class="connection-section" id="connection-auth" data-settings-section="auth" ${settingsTab==='auth'?'':'hidden'}>
            <div class="connection-title-row"><div><h2>禅道连接</h2><div class="connection-version">开源版 22.0 <span>·</span> REST API</div></div><span class="badge">${ico('circle', 9)} 未连接</span></div>
            <form id="connection-form" class="connection-form">
              <div class="connection-field"><label for="zentao-base-url">服务地址</label><input id="zentao-base-url" name="baseUrl" type="url" placeholder="https://zentao.company.com" required autocomplete="url"><small>填写公司禅道的完整访问地址，包含子目录。</small></div>
              <div class="connection-form-grid"><div class="connection-field"><label for="zentao-account">账号</label><input id="zentao-account" name="account" placeholder="你的禅道账号" autocomplete="username"></div><div class="connection-field"><div class="token-label-row"><label for="zentao-token">访问令牌</label><button type="button" class="icon-btn token-help-button" id="zentao-token-help-button" title="如何获取访问令牌" aria-label="如何获取访问令牌" aria-expanded="false" aria-controls="zentao-token-help">${ico('circle-alert', 15)}</button>
                <div class="token-help-panel" id="zentao-token-help" role="region" aria-labelledby="token-help-title" hidden>
                  <div class="token-help-heading"><h3 id="token-help-title">获取访问令牌</h3><button type="button" class="icon-btn" data-close-token-help title="关闭令牌帮助" aria-label="关闭令牌帮助">${ico('x', 15)}</button></div>
                  <p>通过禅道 REST API v1，使用你的禅道账号和密码获取。</p>
                  <ol><li>在 Postman 或其他 API 工具中新建 <code>POST</code> 请求，地址为：<code class="token-help-endpoint">禅道服务地址/api.php/v1/tokens</code><span>保留服务地址中的子目录，例如 <code>/zentao</code>。</span></li>
                  <li>请求头设置 <code>Content-Type: application/json</code>，请求体选择 JSON，填写：<pre><code>{
  "account": "你的禅道账号",
  "password": "你的禅道密码"
}</code></pre></li>
                  <li>发送请求，将返回 JSON 中 <code>token</code> 字段的值填入此处，不包含引号。</li></ol>
                  <p>令牌失效后重新获取。请使用自己的账号，不要分享密码或令牌。</p>
                  <a class="text-btn" href="https://www.zentao.net/book/api/1397.html" target="_blank" rel="noopener noreferrer">查看禅道官方说明 ${ico('external-link', 13)}</a>
                </div>
              </div><input id="zentao-token" name="token" type="password" placeholder="输入访问令牌" required autocomplete="off"></div></div>
              <div class="connection-actions"><button class="btn primary" type="submit" data-action="test-connection">${ico('unplug', 16)} 测试连接</button><span class="muted" style="font-size:12px">尚未验证连接与账号权限</span></div>
            </form>
          </section>
          <section class="connection-section" id="connection-scope" data-settings-section="scope" ${settingsTab==='scope'?'':'hidden'}><h2>同步范围</h2><p>选择项目与执行，加入个人工作台。</p><div class="connection-scope">${ico('folder-sync', 19)}<div><strong>连接后选择需要关注的禅道项目</strong><br>先同步项目，再关联其执行与任务；所属项目保持与禅道一致，个人排期与工作台状态单独保存。</div></div><a class="text-btn" href="#projects">查看项目 ${ico('arrow-up-right')}</a></section>
          <section class="connection-section" id="connection-write" data-settings-section="write" ${settingsTab==='write'?'':'hidden'}><h2>管理权限</h2><div class="connection-switch-row"><div><label class="connection-switch-label" for="management-toggle">允许在工作台中操作禅道</label><p>开启后，显示当前账号具备权限的远端操作。</p></div><input class="connection-switch" type="checkbox" id="management-toggle" role="switch" aria-label="允许在工作台中操作禅道"></div>
            <div class="connection-permissions"><span class="connection-permission">${ico('check', 15)} 本地拖动不回写禅道</span><span class="connection-permission">${ico('check', 15)} 远端操作前明确确认</span><span class="connection-permission">${ico('check', 15)} 本地与禅道状态分别显示</span><span class="connection-permission">${ico('check', 15)} 操作权限以连接核验为准</span></div>
            <div class="connection-footnote">${ico('info', 16)}<span>开源版 22.0 的执行管理与任务流转能力将在连接时核验；只有已验证支持的操作才会开放。</span></div>
          </section>
        </div>
      </div>`;
  }

  window.secondaryPages = { apps, settings, settingsTabs, activateSettingsTab };
})();
