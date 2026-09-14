const repo='tangjiale/perch';
const $=s=>document.querySelector(s);
$('#theme-toggle').addEventListener('click',()=>{document.documentElement.classList.toggle('dark');localStorage.setItem('perch-theme',document.documentElement.classList.contains('dark')?'dark':'light')});
if(localStorage.getItem('perch-theme')==='dark')document.documentElement.classList.add('dark');
function setLink(id,url){const a=$(id);if(url){a.href=url;a.classList.remove('disabled')}else{a.href='#';a.classList.add('disabled')}}
fetch(`https://api.github.com/repos/${repo}/releases/latest`).then(r=>{if(!r.ok)throw Error();return r.json()}).then(r=>{const mac=r.assets.find(a=>/aarch64.*\.dmg$/i.test(a.name));const win=r.assets.find(a=>/x64-setup\.exe$/i.test(a.name));setLink('#mac-download',mac?.browser_download_url);setLink('#win-download',win?.browser_download_url);$('#release-status').textContent=`最新版本 ${r.tag_name} · ${new Date(r.published_at).toLocaleDateString('zh-CN')}`;$('#release-link').href=r.html_url}).catch(()=>{$('#release-status').textContent='暂时无法获取最新版本，请前往 Release 页面下载';});
