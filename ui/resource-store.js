/* 资源只保留在预览会话中；文档不标记为已解析或已向量化。 */
window.resourceStore = {
  skills: [
    {id:'skill-planning',name:'任务拆解',description:'将工作目标整理为任务、依赖和验收标准。',content:'先确认目标与约束，再拆解任务。每项任务包含目标、前置依赖和可验证的验收标准。缺失信息标记为待确认。',source:'local',enabled:true},
    {id:'skill-notes',name:'会议纪要',description:'整理讨论结论、行动项与待确认问题。',content:'依据用户提供的会议内容整理：讨论主题、决策、行动项、负责人和期限。不要补写未提供的事实。',source:'local',enabled:true},
    {id:'skill-review',name:'代码审查',description:'检查边界条件、行为回归和测试缺口。',content:'审查用户提供的代码。优先列出可复现的问题，说明影响和修复建议；区分确定缺陷与需要验证的假设。',source:'imported',enabled:false}
  ],
  knowledgeBases: [
    {id:'kb-work',name:'工作资料',description:'项目方案、会议纪要与团队协作资料。',providerId:'local',modelId:'qwen-embed',documents:[]},
    {id:'kb-tech',name:'技术文档',description:'接口说明、开发规范与技术参考。',providerId:'local',modelId:'qwen-embed',documents:[]}
  ],
  embeddingModels() {
    return aiStore.providers.filter(provider=>provider.enabled).flatMap(provider=>provider.models.filter(model=>model.enabled&&model.type==='embedding').map(model=>({provider,model})));
  },
  resolveEmbedding(kb) {
    const provider=aiStore.providers.find(item=>item.id===kb?.providerId);
    const model=provider?.models.find(item=>item.id===kb?.modelId);
    return {provider,model,available:!!(provider?.enabled&&model?.enabled&&model.type==='embedding')};
  },
  findKnowledge(id) { return this.knowledgeBases.find(kb=>kb.id===id); }
};
aiStore.agents.find(agent=>agent.id==='planner').skillIds=['skill-planning'];
aiStore.agents.find(agent=>agent.id==='writer').skillIds=['skill-notes'];
