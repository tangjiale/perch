/* 项目关系使用稳定 ID；名称仅用于展示，禅道对象保留原始项目标识。 */
window.projectStore = {
  projects: [
    {id:'zt-project-12',name:'客户门户',description:'客户工作台、权限与业务流程的持续迭代。',source:'zentao',connectionId:'zentao-demo',remoteId:12,status:'doing',owner:'我',due:'2026-09-30'},
    {id:'local-workbench',name:'个人工作台',description:'整合任务、日历、应用与 AI 助手。',source:'local',status:'doing',owner:'我',due:'2026-09-25'},
    {id:'zt-project-18',name:'数据服务',description:'统一数据接口，完善数据质量与报表能力。',source:'zentao',connectionId:'zentao-demo',remoteId:18,status:'todo',owner:'我',due:'2026-09-30'},
    {id:'zt-project-23',name:'运维平台',description:'服务监控、指标采集与告警策略建设。',source:'zentao',connectionId:'zentao-demo',remoteId:23,status:'doing',owner:'我',due:'2026-09-20'},
    {id:'local-team',name:'团队协作',description:'工作计划、项目复盘与团队文档。',source:'local',status:'doing',owner:'我',due:''},
    {id:'local-product',name:'产品规划',description:'产品需求、评审材料与阶段规划。',source:'local',status:'todo',owner:'我',due:'2026-09-18'},
    {id:'zt-project-9',name:'基础服务',description:'统一认证与基础能力升级。',source:'zentao',connectionId:'zentao-demo',remoteId:9,status:'done',owner:'我',due:'2026-09-04'}
  ],
  seedTaskProjects: {'T-101':'zt-project-12','T-102':'local-product','E-240':'zt-project-18','T-104':'local-team','E-238':'zt-project-12','T-106':'local-workbench','T-107':'zt-project-12','E-231':'zt-project-23','T-109':'local-team','E-226':'zt-project-9','T-111':'local-team','T-112':'zt-project-18'},
  find(id) { return this.projects.find(p=>p.id===id); },
  remoteProject(connectionId,remoteId) { return this.projects.find(p=>p.source==='zentao'&&p.connectionId===connectionId&&p.remoteId===remoteId); },
  bindInitialTasks(items) {
    return items.map(item=>{
      const projectId=this.seedTaskProjects[item.id]||null, project=this.find(projectId);
      return {...item,projectId,...(item.source==='zentao'?{connectionId:project.connectionId,remoteProjectId:project.remoteId,remoteType:'execution',remoteExecutionId:Number(item.id.slice(2))}:{})};
    });
  }
};
