const RELEASE_REPO = 'tangjiale/perch';
const RELEASE_LATEST = `https://github.com/${RELEASE_REPO}/releases/latest`;

// 只接受当前仓库、当前版本下真实列出的安装包地址。
function releaseAssetUrl(value, tag) {
  try {
    const url = new URL(value);
    const prefix = `/${RELEASE_REPO}/releases/download/${tag}/`;
    return url.protocol === 'https:' && url.hostname === 'github.com' && !url.username && !url.password && !url.search && !url.hash && url.pathname.startsWith(prefix) && url.pathname.slice(prefix.length).length > 0 ? url.href : null;
  } catch { return null; }
}
function releaseInfo(data, fallback = false) {
  const tag = fallback ? `v${String(data.version || '').replace(/^v/, '')}` : data.tag_name;
  if (!/^v\d+\.\d+\.\d+$/.test(tag || '') || (!fallback && (data.draft !== false || data.prerelease !== false))) throw Error('无效正式版本');
  const assets = fallback ? Object.values(data.platforms || {}).map(p => ({ browser_download_url: p.url })) : data.assets;
  if (!Array.isArray(assets)) throw Error('缺少安装包');
  const urls = assets.map(a => releaseAssetUrl(a.browser_download_url, tag)).filter(Boolean);
  const version = tag.slice(1).replace(/\./g, '\\.');
  const mac = urls.find(u => new RegExp(`/[^/]*${version}[^/]*aarch64\\.dmg$`, 'i').test(u)) || null;
  const win = urls.find(u => new RegExp(`/[^/]*${version}[^/]*x64[^/]*-setup\\.exe$`, 'i').test(u)) || null;
  const date = new Date(fallback ? data.pub_date : data.published_at);
  return { tag, mac, win, date: Number.isNaN(date.getTime()) ? '' : date.toLocaleDateString('zh-CN'), fallback };
}
async function fetchRelease(fetcher = fetch) {
  async function get(url) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 8000);
    try {
      const response = await fetcher(url, { cache: 'no-store', signal: controller.signal, headers: { Accept: 'application/json' } });
      if (!response.ok) throw Error('版本请求失败');
      return await response.json();
    } finally { clearTimeout(timer); }
  }
  try { return releaseInfo(await get(`https://api.github.com/repos/${RELEASE_REPO}/releases/latest`)); }
  catch { return releaseInfo(await get('latest.json'), true); }
}
if (typeof module !== 'undefined') module.exports = { releaseAssetUrl, releaseInfo, fetchRelease };
if (typeof document !== 'undefined') {
  const $ = s => document.querySelector(s);
  try { if (localStorage.getItem('perch-theme') === 'dark') document.documentElement.classList.add('dark'); } catch { /* 隐私模式仍可切换主题。 */ }
  $('#theme-toggle').addEventListener('click', () => {
    document.documentElement.classList.toggle('dark');
    try { localStorage.setItem('perch-theme', document.documentElement.classList.contains('dark') ? 'dark' : 'light'); } catch { /* 存储不可用不阻断交互。 */ }
  });
  let pending = false;
  let lastRefresh = 0;
  function setLink(selector, url, availableLabel, missingLabel) {
    const link = $(selector);
    link.href = url || RELEASE_LATEST;
    link.textContent = url ? availableLabel : missingLabel;
    link.classList.remove('disabled');
  }
  async function refreshRelease() {
    if (pending) return;
    pending = true;
    lastRefresh = Date.now();
    try {
      const release = await fetchRelease();
      $('#release-status').textContent = `${release.fallback ? '最近发布记录' : '最新版本'} ${release.tag}${release.date ? ` · ${release.date}` : ''}${release.fallback ? '（实时查询暂不可用）' : ''}`;
      setLink('#mac-download', release.mac, 'macOS 下载 · Apple 芯片', 'macOS · 前往最新 Release');
      setLink('#win-download', release.win, 'Windows 下载 · x64', 'Windows · 前往最新 Release');
      $('#release-link').href = RELEASE_LATEST;
    } catch {
      $('#release-status').textContent = '暂时无法查询版本，请前往最新 Release 下载';
      setLink('#mac-download', null, '', 'macOS · 前往最新 Release');
      setLink('#win-download', null, '', 'Windows · 前往最新 Release');
    } finally { pending = false; }
  }
  refreshRelease();
  document.addEventListener('visibilitychange', () => { if (!document.hidden) refreshRelease(); });
  window.addEventListener('pageshow', event => { if (event.persisted) refreshRelease(); });
  setInterval(() => { if (!document.hidden && Date.now() - lastRefresh >= 300000) refreshRelease(); }, 60000);
}
