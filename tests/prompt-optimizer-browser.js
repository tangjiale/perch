// 模拟 IPC 验收；不读取真实 SQLite、不请求真实模型。持久化仅测试 fixture 自有 localStorage。
import {execFileSync} from 'node:child_process';
import {mkdirSync} from 'node:fs';
import assert from 'node:assert/strict';
const origin=process.argv[2]||'http://127.0.0.1:1427';
const run=(...args)=>execFileSync('npx',['--no-install','agent-browser','--session','prompt-check',...args],{encoding:'utf8',timeout:60000});
const ev=code=>JSON.parse(JSON.parse(run('eval',`JSON.stringify(${code})`).trim()));
const wait=()=>run('wait','180');
const open=(query='')=>{run('open',`${origin}/tests/prompt-optimizer.html${query}`);wait()};
const click=text=>{ev(`(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent.trim()===${JSON.stringify(text)});if(!b)throw Error('missing '+${JSON.stringify(text)});b.click();return true})()`);wait()};
const fill=(selector,text)=>{run('fill',selector,text);wait()};
const val=selector=>ev(`document.querySelector(${JSON.stringify(selector)})?.value`);
const body=()=>ev('document.body.textContent');
const title=()=>'.prompt-editor form > label input';
let count=0;
const check=(name,fn)=>{fn();count++;console.log('✓ '+name)};
mkdirSync('/tmp/perch-prompt-qa',{recursive:true});
if(process.argv.includes('--mutation')){
try{
open();ev(`(()=>{window.__entries=[{id:'race-entry',title:'并发测试提示词',content:'你是工作周报助手',kind:'system',tags:['工作'],favorite:false,versions:[],revision:1}];window.__persist();return true})()`);open();click('我的提示词 1');
ev(`window.__mode='list-pending'`);run('click','[aria-label="刷新提示词库"]');wait();run('click','[title="收藏"]');wait();ev(`(()=>{window.__pendingLists.at(-1).resolve();return true})()`);wait();
check('迟到旧列表不撤销刚更新的收藏',()=>assert.equal(ev('document.querySelector(".prompt-library-star").getAttribute("aria-pressed")'),'true'));
run('click','[aria-label="刷新提示词库"]');wait();run('click','[title="删除提示词"]');wait();click('确认删除');ev(`(()=>{window.__pendingLists.at(-1).resolve();return true})()`);wait();
check('迟到旧列表不恢复已删除提示词',()=>{assert.equal(ev('document.querySelectorAll(".prompt-library-list article").length'),0);assert.match(body(),/把好用的提示词留在这里/)});
open();fill('#prompt-source','你是工作周报助手，请保留 {{主题}}，输出本周完成事项、风险和下周安排。');click('开始优化');run('set','viewport','900','800');
for(const theme of ['light','dark']){ev(`document.documentElement.dataset.theme='${theme}'`);wait();check(`900×800 ${theme} 内容区无横溢且主操作在首屏`,()=>{assert.equal(ev('document.querySelector("#root").scrollWidth<=document.querySelector("#root").clientWidth'),true);assert.ok(ev(`[...document.querySelectorAll('button')].find(x=>x.textContent.trim()==='开始优化').getBoundingClientRect().bottom`)<=800)});run('screenshot',`/tmp/perch-prompt-qa/workbench-900-${theme}.png`)}
console.log(`PASS ${count} mutation checks`);
}catch(error){console.error(error);process.exitCode=1}finally{run('close')}
process.exit(process.exitCode||0);
}
try{
open();ev(`(()=>{localStorage.removeItem('perch-prompt-fixture');return true})()`);open('?index');
check('工具列表有提示词优化入口',()=>assert.match(body(),/提示词优化/));
ev(`(()=>{[...document.querySelectorAll('.tool-card')].find(x=>x.textContent.includes('提示词优化')).click();return true})()`);wait();
}catch(error){
// 工具入口是组合卡片，按标题定位。
if(String(error).includes('missing')){ev(`(()=>{[...document.querySelectorAll('.tool-card')].find(x=>x.textContent.includes('提示词优化')).click();return true})()`);wait()}else throw error;
}
try{
fill('#prompt-source','周报 {{主题}}');click('开始优化');
check('优化完成保留原文与变量',()=>{assert.equal(val('#prompt-source'),'周报 {{主题}}');assert.match(val('#prompt-output'),/优化后的提示词：周报 {{主题}}/)});
check('展示供应商用量与耗时',()=>assert.match(body(),/0.9 秒.*30 tokens/s));
fill('#prompt-instruction','更精简');click('继续迭代');check('继续迭代采用上次结果',()=>assert.match(val('#prompt-output'),/^迭代后的提示词：优化后的提示词/));
const old=val('#prompt-output');ev(`window.__mode='fail'`);click('开始优化');check('失败保留原文及已完成结果',()=>{assert.equal(val('#prompt-output'),old);assert.equal(val('#prompt-source'),'周报 {{主题}}');assert.match(body(),/模拟模型调用失败/)});
ev(`window.__mode='pending'`);click('开始优化');ev(`(()=>{window.__pending.at(-1).emit('正在生成的部分内容');return true})()`);wait();check('显示流式累计内容',()=>assert.equal(val('#prompt-output'),'正在生成的部分内容'));
click('停止生成');check('停止恢复上次完成结果',()=>assert.equal(val('#prompt-output'),old));
ev(`(()=>{const p=window.__pending.at(-1);p.resolve(window.__result(p.r));return true})()`);wait();check('停止后迟到成功不覆盖结果',()=>assert.equal(val('#prompt-output'),old));
ev(`window.__mode='success'`);click('开始优化');
ev(`(()=>{document.querySelector('.prompt-test').open=true;return true})()`);wait();fill('#prompt-test-input','本周完成了登录模块');click('开始对比');check('原始与优化回答均完成',()=>{assert.equal(ev('document.querySelectorAll(".prompt-comparison section").length'),2);assert.equal(ev('document.querySelectorAll(".prompt-comparison section .prompt-run-meta").length'),2)});
ev(`window.__mode='test-partial'`);click('开始对比');check('对比单侧失败保留另一侧成功',()=>{assert.match(body(),/模拟单侧测试失败/);assert.equal(ev('document.querySelectorAll(".prompt-comparison section .prompt-run-meta").length'),1)});
click('保存到提示词库');fill(title(),'工作周报');fill('.prompt-editor-meta input:not([type=checkbox])','工作，写作');ev(`window.__mode='save-fail'`);click('保存提示词');check('保存失败保留编辑草稿',()=>{assert.equal(val(title()),'工作周报');assert.match(body(),/模拟保存失败/)});
ev(`window.__mode='success'`);click('保存提示词');click('我的提示词 1');check('保存进入个人提示词库',()=>assert.match(body(),/工作周报/));
fill('input[aria-label="搜索提示词"]','不存在');check('搜索无结果空态',()=>assert.match(body(),/没有符合条件/));fill('input[aria-label="搜索提示词"]','工作');check('标题标签搜索',()=>assert.equal(ev('document.querySelectorAll(".prompt-library-list article").length'),1));
run('click','[title="收藏"]');wait();click('只看收藏');check('收藏筛选',()=>assert.equal(ev('document.querySelectorAll(".prompt-library-list article").length'),1));
run('click','[title="编辑提示词"]');wait();fill('.prompt-editor textarea','修改版 {{主题}}');click('保存提示词');run('click','[title="编辑提示词"]');wait();ev(`(()=>{document.querySelector('.prompt-version-history').open=true;return true})()`);wait();
check('正文编辑生成版本记录',()=>assert.match(body(),/2 个版本/));
run('focus','.prompt-version-controls [role=combobox]');run('press','Space');wait();run('press','End');run('press','Enter');wait();click('还原到编辑区');check('历史版本仅还原编辑草稿',()=>{assert.match(val('.prompt-editor textarea'),/^优化后的提示词/);assert.equal(ev('window.__entries[0].content'),'修改版 {{主题}}')});click('取消');
run('click','[title="删除提示词"]');wait();check('删除前显示明确确认',()=>assert.match(body(),/确认删除/));click('取消');check('取消删除保留记录',()=>assert.equal(ev('window.__entries.length'),1));
run('click','[title="载入优化器"]');wait();check('库载入工作台',()=>assert.equal(val('#prompt-source'),'修改版 {{主题}}'));
open();click('我的提示词 1');check('页面重载恢复 mock 持久记录',()=>assert.match(body(),/工作周报/));
run('click','[title="删除提示词"]');wait();ev(`window.__mode='delete-fail'`);click('确认删除');check('删除失败保留确认与记录',()=>{assert.match(body(),/模拟删除失败/);assert.equal(ev('window.__entries.length'),1)});ev(`window.__mode='success'`);click('确认删除');check('明确确认删除完成',()=>assert.equal(ev('window.__entries.length'),0));
click('新建提示词');fill(title(),'手动创建');fill('.prompt-editor textarea','你是写作助手');click('保存提示词');check('手动新建提示词',()=>assert.equal(ev('window.__entries[0].title'),'手动创建'));
open('?no-models');check('无模型引导配置且禁止优化',()=>{assert.match(body(),/模型管理/);assert.equal(ev(`[...document.querySelectorAll('button')].find(x=>x.textContent.trim()==='开始优化').disabled`),true)});
open('?preview');check('浏览器预览不伪保存',()=>{assert.match(body(),/浏览器预览不保存数据/);assert.equal(ev('window.__calls.length'),0)});
open();fill('#prompt-source','你是工作周报助手，请保留 {{主题}}，输出本周完成事项、风险和下周安排。');click('开始优化');
for(const [width,height] of [[1440,1000],[900,800]])for(const theme of ['light','dark']){run('set','viewport',String(width),String(height));ev(`document.documentElement.dataset.theme='${theme}'`);wait();check(`${width}×${height} ${theme} 无横向溢出`,()=>assert.equal(ev('document.documentElement.scrollWidth<=innerWidth&&document.querySelector("#root").scrollWidth<=document.querySelector("#root").clientWidth'),true));run('screenshot',`/tmp/perch-prompt-qa/workbench-${width}-${theme}.png`)}
click('我的提示词 1');run('screenshot','/tmp/perch-prompt-qa/library-900-dark.png');
console.log(`PASS ${count} checks; screenshots /tmp/perch-prompt-qa/`);
}catch(error){console.error(`FAIL after ${count} checks`,error);process.exitCode=1}finally{run('close')}
