/** 视频交互演示：只在内存处理白名单 IPC，不读取真实工作空间，也不连接外部服务。 */
import type { Snapshot, Task, Conversation } from "../src/lib/types";
import { emptySnapshot } from '../src/lib/api';
const day='2026-10-16';
const now=new Date(`${day}T10:30:00+08:00`).getTime();
const projects=[{id:'p1',name:'栖点 · 秋季体验升级',description:'让日常工作更清晰、更从容',source:'zentao',status:'doing',owner:'演示成员',remoteId:'21',connectionId:'demo',plannedStartDate:'2026-10-01',plannedEndDate:'2026-10-30',dueDate:'2026-10-30'},{id:'p2',name:'个人学习计划',description:'每周留一点时间，积累新的能力',source:'local',status:'doing',owner:'我',dueDate:'2026-10-31'}];
const titles=['梳理本周产品迭代目标','完善任务提醒交互','整理季度分享材料','确认日历接入范围','更新设计组件文档','完成邮件阅读体验优化','整理官网产品截图','回顾上周发布反馈'];
const tasks=titles.map((title,i)=>({id:`t${i}`,revision:1,title,notes:'<p>让每一个细节都服务于清晰、顺畅的工作体验。</p>',source:i%2?'local':'zentao',projectId:i%3?'p1':'p2',status:['todo','doing','todo','suspended','doing','done','todo','done'][i],priority:i===1?'high':i===4?'low':'normal',sortOrder:i*1024,createdAt:now-86400000*(i+1),updatedAt:now,completedAt:i===5||i===7?now:null,schedule:{kind:'all_day',start:`2026-10-${String(13+i%6).padStart(2,'0')}`,end:`2026-10-${String(15+i%6).padStart(2,'0')}`,timezone:'Asia/Shanghai'},reminder:{enabled:i<3,advanceMinutes:15},...(i%2?{}:{remoteId:String(101+i),remoteType:'execution',connectionId:'demo',remoteStatus:'doing'})}));
const provider={id:'provider',name:'演示模型服务',baseUrl:'https://models.example.test/v1',protocol:'openai-completions',enabled:true};
const model={id:'vision',name:'视觉工作助手',remoteModelId:'demo-vision',providerId:'provider',capability:'vision',enabled:true};
const agent={id:'assistant',name:'工作伙伴',modelId:'vision',systemPrompt:'帮助整理工作思路',temperature:0.7,maxTokens:4096,enabled:true,skillIds:[]};
const bugs=['移动端筛选面板间距调整','修正日历跨月排期显示','优化暗色主题对比度','修复邮件附件名称换行','统一列表空状态文案'].map((title,i)=>({id:`b${i}`,connectionId:'demo',remoteId:String(301+i),revision:1,title,steps:'<p>在对应页面检查不同窗口尺寸下的交互与显示。</p>',status:['active','active','resolved','closed','resolved'][i],confirmed:true,assignedTo:'demo',assignedToName:'演示成员',lastAssignedTo:'demo',severity:i===1?2:3,priority:i<2?2:3,productName:'栖点工作台',projectId:'21',projectName:projects[0].name,openedDate:`2026-10-${String(10+i).padStart(2,'0')} 09:30:00`,resolvedDate:i>1?'2026-10-15 10:00:00':undefined}));
const snapshot={...emptySnapshot,projects,tasks,bugs,connections:[{id:'demo',name:'团队禅道 · 演示',baseUrl:'https://zentao.example.test',apiVersion:'v1',enabled:true,managementEnabled:true,hasCredential:true,lastSync:now,syncIntervalMinutes:5}],categories:[{id:'work',name:'工作协作'},{id:'learn',name:'学习与灵感'}],apps:[['团队文档','https://docs.example.test','共享知识与协作记录','work'],['设计资源','https://design.example.test','常用组件与设计参考','work'],['开发手册','https://developer.example.test','技术文档随手查阅','learn'],['阅读清单','https://read.example.test','留给深度阅读的时间','learn']].map(([name,url,description,categoryId],i)=>({id:`app${i}`,name,url,description,categoryId,favorite:true,sortOrder:i})),providers:[provider],models:[model],agents:[agent],knowledge:[{id:'knowledge',name:'产品知识库',description:'产品说明与团队协作资料',embeddingModelId:'embedding'}],documents:[{id:'doc',knowledgeId:'knowledge',name:'产品设计原则.md',status:'ready',chunks:12}],conversations:[{id:'chat',title:'整理本周迭代重点',agentId:agent.id,knowledgeId:'knowledge',createdAt:now,updatedAt:now,agentSnapshot:{...agent,model,provider}}],messages:[{id:'m1',conversationId:'chat',role:'user',content:'帮我把本周的迭代目标整理成一份清晰的行动清单。',status:'completed',createdAt:now-60000},{id:'m2',conversationId:'chat',role:'assistant',content:'## 本周聚焦：把工作衔接得更顺畅\n\n结合产品设计原则，建议优先推进以下三个方向：\n\n### 1. 让安排更清晰\n- 完善任务到期提醒，覆盖提前提醒与到期时通知。\n- 核对日历排期，让今日安排与看板保持一致。\n\n### 2. 让协作有依据\n- 核实禅道执行的指派信息与 BUG 操作历史。\n- 为关键交互补充验证，记录真实结果。\n\n### 3. 让数据更安心\n- 检查 S3 定时备份与保留策略。\n- 更新官网截图，让新用户看见当前的栖点。\n\n建议今天先完成 **任务提醒与日历排期的联动验证**，再整理发布材料。',status:'completed',createdAt:now,finishedAt:now,durationMs:1850,usage:{inputTokens:862,outputTokens:436,totalTokens:1298},citations:[{documentId:'doc',name:'产品设计原则.md',excerpt:'让清晰与可靠贯穿日常工作。',locator:'第 2 节'}]}]};
const demo = snapshot as unknown as Snapshot;
const calendar={config:{revision:1,serverUrl:'https://calendar.example.test',username:'演示账号',enabled:true,syncIntervalMinutes:5,calendarUrl:'https://calendar.example.test/demo/',calendarName:'工作日历',hasCredential:true},calendars:[],events:[{id:'e1',title:'产品迭代周会',description:'同步本周目标与进展',location:'线上会议',start:`${day}T14:00:00+08:00`,end:`${day}T15:00:00+08:00`,allDay:false},{id:'e2',title:'设计方案评审',description:'',location:'协作空间',start:'2026-10-16T10:00:00+08:00',end:'2026-10-16T11:00:00+08:00',allDay:false}],todos:[],lastSync:`${day}T10:00:00+08:00`};
const backup={config:{revision:1,endpoint:'https://s3.example.test',region:'auto',bucket:'perch-backups',prefix:'workspace/',forcePathStyle:false,hasCredentials:true,enabled:true,cron:'0 2 * * *',retentionDays:14,maxBackups:10,monthlyArchive:true},records:[],nextRunAt:'2026-10-17T02:00:00+08:00',running:false};
// 不接触真实 IPC。只接受本地任务与无秘密会话元数据；远端对象写入始终拒绝。
function saveMemory<T extends Task | Conversation>(items: T[], value: T): T {
 if (!value || typeof value.id !== 'string' || !value.id) throw Error('演示记录缺少编号');
 const index=items.findIndex(item=>item.id===value.id);
 const previous=items[index];
 if(previous && (previous.revision||0)!==(value.revision||0)) throw Error('演示记录已更新，请刷新');
 const saved={...structuredClone(value),revision:(previous?.revision||0)+1,createdAt:previous?.createdAt||Date.now(),updatedAt:Date.now()};
 if(index<0)items.push(saved);else items[index]=saved;
 return structuredClone(saved);
}
const internal=(window as unknown as {__TAURI_INTERNALS__:{invoke:(name:string,args?:Record<string,unknown>)=>Promise<unknown>}}).__TAURI_INTERNALS__;
internal.invoke=async(name:string,args:Record<string,unknown>={})=>{
 if(name==='workspace_snapshot')return structuredClone(demo);
 if(name==='save_task'){
  const value=structuredClone(args.value) as Task & {boardBeforeId?:string|null};
  if(value.source!=='local')throw Error('演示禁止修改禅道执行');
  if(!value.title?.trim()||!['todo','doing','suspended','done'].includes(value.status))throw Error('请填写有效任务');
  if(value.projectId && demo.projects.find(project=>project.id===value.projectId)?.source==='zentao')throw Error('演示请选未关联项目或个人学习计划');
  const beforeId=value.boardBeforeId;
  delete value.boardBeforeId;
  const previous=demo.tasks.find(task=>task.id===value.id);
  value.completedAt=value.status==='done'?(previous?.completedAt||Date.now()):null;
  const result=saveMemory(demo.tasks,value);
  if(beforeId!==undefined){
   const siblings=demo.tasks.filter(task=>task.status===result.status&&task.id!==result.id).sort((a,b)=>a.sortOrder-b.sortOrder||a.id.localeCompare(b.id));
   const before=beforeId===null?-1:siblings.findIndex(task=>task.id===beforeId);
   siblings.splice(before<0?siblings.length:before,0,demo.tasks.find(task=>task.id===result.id)!);
   siblings.forEach((task,index)=>{task.sortOrder=(index+1)*1024;});
   return structuredClone(demo.tasks.find(task=>task.id===result.id));
  }
  return result;
 }
 if(name==='save_conversation')return saveMemory(demo.conversations,args.value as Conversation);
 if(name==='chat_set_model'){
  const current=demo.conversations.find(item=>item.id===args.conversationId);
  const model=demo.models.find(item=>item.id===args.modelId&&item.enabled);
  const provider=demo.providers.find(item=>item.id===model?.providerId&&item.enabled);
  if(!current||!model||!provider||!current.agentSnapshot)throw Error('演示会话配置不可用');
  return saveMemory(demo.conversations,{...current,revision:args.revision as number|undefined,agentSnapshot:{...current.agentSnapshot,model,provider}});
 }
 if(name==='chat_send')throw Error('演示数据：不会发送到模型服务，请查看已有示例对话');
 // 仅模拟表单权限预检；不请求系统权限、不注册提醒、不发送系统通知。
 if(name==='task_reminder_permission')return true;
 if(name==='storage_info')return {dataRoot:'/demo/perch',defaultRoot:'/demo/perch',workspaceId:'video-demo',version:'0.1.19'};
 if(name==='mail_unread')return {total:3,accounts:[]};
 if(name==='mail_accounts'||name==='mcp_list'||name==='prompt_list')return [];
 if(name==='dingtalk_calendar_state')return structuredClone(calendar);
 if(name==='backup_state')return structuredClone(backup);
 if(name==='updater_status')return {configured:false,releasesUrl:null};
 if(name==='get_exchange_rates'){
  const reference:Record<string,number>={CNY:1,USD:0.1404,EUR:0.1291,JPY:20.83,HKD:1.093,GBP:0.108,AUD:0.212,CAD:0.193,SGD:0.183,CHF:0.122,KRW:190.3,NZD:0.231,TWD:4.52,THB:4.72,MYR:0.61,IDR:2238,INR:11.8,AED:0.516,MOP:1.126,VND:3510};
  const base=typeof args.base==='string'?args.base:'CNY';
  if(!reference[base])throw Error('演示不支持该币种');
  const rates=Object.fromEntries(Object.entries(reference).map(([code,rate])=>[code,rate/reference[base]]));
  return {base,rates,updatedAt:`${day}T10:00:00+08:00`,nextUpdateAt:'2026-10-17T10:00:00+08:00',fetchedAt:`${day}T10:30:00+08:00`,provider:'演示汇率数据'};
 }
 if(name==='plugin:event|listen')return 1;
 if(['plugin:window|set_title','plugin:window|is_maximized','plugin:window|is_focused','plugin:window|scale_factor','plugin:event|unlisten'].includes(name))return null;
 throw Error('隔离演示禁止命令：'+name);
};
localStorage.setItem('perch.appearance.v1',JSON.stringify({mode:'light',chatFontSize:14}));
await import('../src/main');
const label=document.createElement('span');label.textContent='演示数据';label.style.cssText='position:fixed;bottom:6px;left:50%;transform:translateX(-50%);font-size:10px;color:#60776d;z-index:200;pointer-events:none';document.body.append(label);
