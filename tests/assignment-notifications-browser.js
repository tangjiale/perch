// 仅使用模拟 IPC，不连接真实禅道。
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
const origin=process.argv[2]||'http://127.0.0.1:1420';
const run=(...args)=>execFileSync('npx',['--yes','agent-browser','--session','assignment-inbox-test',...args],{encoding:'utf8',timeout:60000});
const ev=code=>JSON.parse(JSON.parse(run('eval',`JSON.stringify(${code})`).trim()));
const wait=()=>run('wait','300');
run('open',`${origin}/tests/assignment-notifications.html`);wait();
assert.equal(ev(`document.querySelector('.assignment-count').textContent`),'2');
run('click','.assignment-bell');wait();
assert.equal(ev(`window.__calls.filter(c=>c.name==='assignment_notification_read').length`),0,'打开列表不清未读');
run('click','[aria-label="关闭提醒列表"]');wait();
ev(`window.__emit('assignment-notification-new',{workspaceId:'workspace-a',items:[window.__items[0]]})`);wait();
assert.equal(ev(`document.querySelectorAll('.assignment-bubble').length`),1);
run('click','[aria-label="关闭气泡，保留未读"]');wait();
assert.equal(ev(`document.querySelector('.assignment-count').textContent`),'2','关闭气泡保留未读');
run('click','.assignment-bell');wait();
ev(`window.__fail=true`);run('click','.assignment-list .assignment-item:first-child');wait();
assert.equal(ev(`document.querySelector('output').textContent`),'/','标读失败不能导航');
assert.equal(ev(`document.querySelector('.assignment-count').textContent`),'2');
ev(`window.__fail=false`);run('click','.assignment-list .assignment-item:first-child');wait();
assert.equal(ev(`document.querySelector('output').textContent`),'/tasks?taskId=t1&notificationId=n1');
assert.equal(ev(`document.querySelector('.assignment-count').textContent`),'1');
assert.equal(ev(`window.__calls.find(c=>c.name==='assignment_notification_read').args.generation`),'g-a','写入携带捕获的generation');
ev(`window.__emit('assignment-notification-new',{workspaceId:'other',items:[window.__items[0]]})`);wait();
assert.equal(ev(`document.querySelectorAll('.assignment-bubble').length`),0,'忽略其他空间事件');
ev(`window.__emit('assignment-notification-open',{workspaceId:'workspace-a',id:'n2',kind:'bug',entityId:'b1'})`);wait();
assert.equal(ev(`document.querySelector('output').textContent`),'/bugs?bugId=b1&notificationId=n2');
for (const kind of ['task','bug']) {
 run('open',`${origin}/tests/assignment-notifications.html?detail=${kind}#/${kind==='task'?'tasks?taskId=t1':'bugs?bugId=b1'}`);wait();
 assert.equal(ev(`document.querySelectorAll('dialog[open]').length`),0,'数据未到达前不打开空详情');
 assert.equal(ev(`window.__calls.filter(c=>c.name==='assignment_notification_read_entity').length`),0,'数据未加载时不清未读');
 ev(`(() => { window.__fail=true;window.__loadDetails();return true;})()`);wait();
 assert.equal(ev(`document.querySelectorAll('dialog[open]').length`),1,'数据到达后定位真实详情');
 assert.equal(ev(`window.__items.length`),2,'详情标读失败保留未读');
 run('click','dialog header button[title="关闭"]');wait();
 ev(`window.__refreshDetails()`);wait();
 assert.equal(ev(`document.querySelectorAll('dialog[open]').length`),0,'自动刷新不重复打开详情');
}
const screenshots=[];
for(const width of [900,1440]) for(const theme of ['light','dark']) {
 run('open',`${origin}/tests/assignment-notifications.html`);wait();run('set','viewport',String(width),'900');
 ev(`(() => { document.documentElement.dataset.theme='${theme}';window.__emit('assignment-notification-new',{workspaceId:'workspace-a',items:window.__items});return true;})()`);wait();
 const bubblePath=`/tmp/perch-assignment-${width}-${theme}-bubbles.png`;run('screenshot',bubblePath);screenshots.push(bubblePath);run('click','.assignment-bell');wait();
 const geometry=ev(`(() => {const nodes=[...document.querySelectorAll('.assignment-popover,.assignment-bubble')];return nodes.map(node=>{const r=node.getBoundingClientRect();return {left:r.left,right:r.right,bottom:r.bottom,width:r.width,scroll:node.scrollWidth,client:node.clientWidth}})})()`);
 assert.ok(geometry.every(r=>r.left>=0&&r.right<=width&&r.bottom<=900&&r.scroll<=r.client),'组件无视口溢出');
 const path=`/tmp/perch-assignment-${width}-${theme}.png`;run('screenshot',path);screenshots.push(path);
 ev(`(() => { window.__items=[];window.__emit('assignment-notifications-updated',{workspaceId:'workspace-a'});return true;})()`);wait();
 assert.equal(ev(`document.querySelectorAll('.assignment-bubble').length`),0,'详情已读刷新后移除气泡');
}
console.log(JSON.stringify({screenshots,deepLinks:true,delayedSnapshot:true,readFailurePreservesUnread:true,noReopenOnRefresh:true,readRemovesBubbles:true}));
console.log('通过：未读计数、列表保留未读、关闭气泡、标读失败、单条标读导航、空间隔离、原生通知导航');run('close');
