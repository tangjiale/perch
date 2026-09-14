const repo='tangjiale/perch';
const $=s=>document.querySelector(s);
$('#theme-toggle').addEventListener('click',()=>{document.documentElement.classList.toggle('dark');localStorage.setItem('perch-theme',document.documentElement.classList.contains('dark')?'dark':'light')});
if(localStorage.getItem('perch-theme')==='dark')document.documentElement.classList.add('dark');
function setLink(id,url){const a=$(id);if(url){a.href=url;a.classList.remove('disabled')}else{a.href='#';a.classList.add('disabled')}}
// latest.json 是 Release 同步发布的静态清单，不受 GitHub REST API 访客限流影响。
fetch(`https://github.com/${repo}/releases/latest/download/latest.json`,{cache:'no-store'}).then(r=>{if(!r.ok)throw Error();return r.json()}).then(r=>{const tag=r.version?.startsWith('v')?r.version:`v${r.version}`;const base=`https://github.com/${repo}/releases/download/${tag}`;setLink('#mac-download',`${base}/Perch_${r.version}_aarch64.dmg`);setLink('#win-download',r.platforms?.['windows-x86_64']?.url||`${base}/Perch_${r.version}_x64-setup.exe`);$('#release-status').textContent=`最新版本 ${tag} · ${new Date(r.pub_date).toLocaleDateString('zh-CN')}`;$('#release-link').href=`https://github.com/${repo}/releases/tag/${tag}`}).catch(()=>{$('#release-status').textContent='暂时无法获取最新版本，请前往 Release 页面下载';});
