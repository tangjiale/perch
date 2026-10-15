// node tests/modal-dismiss-browser.js http://127.0.0.1:1422
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
const origin=process.argv[2]||'http://127.0.0.1:1422';
const run=(...args)=>execFileSync('npx',['--yes','agent-browser','--session','modal-dismiss-test',...args],{encoding:'utf8',timeout:60000});
const evaluate=code=>JSON.parse(JSON.parse(run('eval',`JSON.stringify(${code})`).trim()));
const wait=()=>run('wait','500');
try {
run('open',`${origin}/tests/modal-dismiss.html`);run('set','viewport','1440','900');wait();
run('fill','dialog input[required]','未保存的任务');
run('click','.tiptap');run('type','.tiptap','未保存的备注');wait();
const retained=()=>{assert.equal(evaluate(`!!document.querySelector('dialog[open]')`),true);assert.equal(evaluate(`document.querySelector('dialog input[required]').value`),'未保存的任务');assert.ok(evaluate(`document.querySelector('.tiptap').textContent`).includes('未保存的备注'));};
run('mouse','move','5','5');run('mouse','down');run('mouse','up');wait();retained();
evaluate(`document.querySelector('dialog').click()`);wait();retained();
run('press','Escape');wait();retained();run('press','Escape');wait();retained();
run('click','[role=combobox][aria-label="任务状态"]');wait();run('press','Escape');wait();retained();assert.equal(evaluate(`document.querySelector('[role=combobox][aria-label="任务状态"]').getAttribute('aria-expanded')`),'false');
run('screenshot','/tmp/perch-modal-draft-retained.png');
run('click','dialog button[title="关闭"]');wait();assert.equal(evaluate(`!!document.querySelector('dialog[open]')`),false);
run('open',`${origin}/tests/modal-dismiss.html?busy`);wait();assert.equal(evaluate(`document.querySelector('dialog button[title="关闭"]').disabled`),true);
run('press','Escape');run('press','Escape');evaluate(`document.querySelector('dialog').click()`);wait();assert.equal(evaluate(`!!document.querySelector('dialog[open]')`),true);
console.log(JSON.stringify({outsideRetainsDraft:true,dialogBlankRetainsDraft:true,escapeRetainsDraft:true,selectEscapeOnlyClosesSelect:true,explicitClose:true,busyCannotClose:true}));
} finally {run('close');}
