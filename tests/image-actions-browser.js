// node tests/image-actions-browser.js http://127.0.0.1:1421
// 隔离 IPC 验证，不读取剪贴板、不下载文件、不修改真实禅道。
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
const origin=process.argv[2] || 'http://127.0.0.1:1421';
const run=(...args)=>execFileSync('npx',['--yes','agent-browser','--session','image-actions-test',...args],{encoding:'utf8',timeout:60000});
const evaluate=code=>JSON.parse(JSON.parse(run('eval',`JSON.stringify(${code})`).trim()));
const wait=()=>run('wait','500');
const menu='[role="menu"][aria-label="图片操作"]';
const openMenu=(selector,x=40,y=40)=>{evaluate(`document.querySelector(${JSON.stringify(selector)}).dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,button:2,clientX:${x},clientY:${y}}))`);wait();};
const text=()=>evaluate(`document.querySelector('.image-actions-feedback')?.textContent ?? ''`);
run('open',`${origin}/tests/bugs.html`);run('set','viewport','900','760');wait();run('click','.bugs-title');wait();
openMenu('.bugs-steps img');
assert.deepEqual(evaluate(`[...document.querySelectorAll('${menu} [role=menuitem]')].map(x=>x.textContent)`),['复制图片','下载图片']);
run('press','Escape');assert.equal(evaluate(`document.querySelectorAll('dialog[open]').length`),1);
openMenu('.bugs-steps img',895,755);
const rect=evaluate(`(()=>{const r=document.querySelector('${menu}').getBoundingClientRect();return {left:r.left,right:r.right,top:r.top,bottom:r.bottom};})()`);
assert.ok(rect.left>=0 && rect.top>=0 && rect.right<=900 && rect.bottom<=760,JSON.stringify(rect));
run('click',`${menu} [role=menuitem]:first-child`);wait();assert.equal(text(),'图片已复制');
assert.equal(evaluate(`window.__bugCalls.findLast(x=>x.name==='image_clipboard_write').args.dataUrl`),evaluate('window.__bugImageData'));
openMenu('.bugs-steps img');run('click',`${menu} [role=menuitem]:last-child`);wait();assert.equal(text(),'图片已保存');
assert.equal(evaluate(`window.__bugCalls.findLast(x=>x.name==='image_download').args.dataUrl`),evaluate('window.__bugImageData'));
evaluate(`window.__imageActionMode='cancel'`);openMenu('.bugs-steps img');run('click',`${menu} [role=menuitem]:last-child`);wait();assert.equal(text(),'');
for(const nth of ['first-child','last-child']){evaluate(`window.__imageActionMode='fail'`);openMenu('.bugs-steps img');run('click',`${menu} [role=menuitem]:${nth}`);wait();assert.ok(evaluate(`document.querySelector('.image-actions-feedback[role=alert]')?.textContent.includes('失败')`));}
evaluate(`window.__imageActionMode='pending'`);openMenu('.bugs-steps img');run('click',`${menu} [role=menuitem]:last-child`);wait();assert.equal(text(),'请选择图片保存位置…');
const before=evaluate(`window.__bugCalls.filter(x=>x.name==='image_download').length`);openMenu('.bugs-steps img');assert.equal(evaluate(`document.querySelector('${menu}')===null`),true);assert.equal(evaluate(`window.__bugCalls.filter(x=>x.name==='image_download').length`),before);
evaluate(`(()=>{window.__resolveImageAction(false); window.__imageActionMode=null; return true;})()`);wait();assert.equal(text(),'');
run('click','[aria-label="放大查看复现步骤图片"]');wait();openMenu('.image-viewer-image');run('press','Escape');assert.equal(evaluate(`document.querySelectorAll('dialog[open]').length`),2);run('press','Escape');wait();assert.equal(evaluate(`document.querySelectorAll('dialog[open]').length`),2);run('click','dialog.image-viewer button[title="关闭"]');wait();assert.equal(evaluate(`document.querySelectorAll('dialog[open]').length`),1);
assert.equal(evaluate(`window.__bugCalls.filter(x=>['zentao_bug_save','zentao_bug_transition'].includes(x.name)).length`),0);
run('open',`${origin}/tests/task-rich-text.html?retry`);wait();openMenu('.task-rich-image img');run('click',`${menu} [role=menuitem]:first-child`);wait();assert.equal(text(),'图片已复制');assert.equal(evaluate(`window.richCalls.findLast(x=>x.name==='image_clipboard_write').args.dataUrl`),evaluate('window.richPng'));
assert.equal(evaluate(`window.richCalls.filter(x=>x.name==='save_task').length`),0);
assert.ok(run('eval',readFileSync('tests/task-rich-text-unchanged-browser.js','utf8')).includes('statusOnlyPreservesNotes'));
run('close');run('open',`${origin}/tests/bugs.html`);wait();console.log('BUG 回归:',run('eval',readFileSync('tests/bugs-browser.js','utf8')).trim());
run('open',`${origin}/tests/task-rich-text.html`);wait();console.log('任务富文本回归:',run('eval',readFileSync('tests/task-rich-text-browser.js','utf8')).trim());
console.log(JSON.stringify({chineseMenu:true,copyData:true,saveData:true,cancel:true,errors:true,pending:true,escape:true,edgePosition:true,noAutoSubmit:true,unchangedNotes:true}));
