/* AI 设计预览只保存非敏感配置与演示会话，不存储 API 密钥。 */
window.aiStore = {
  providers: [
    {id:'deepseek',name:'DeepSeek',kind:'preset',baseUrl:'https://api.deepseek.com',protocol:'openai-completions',enabled:true,models:[
      {id:'ds-chat',modelId:'deepseek-chat',name:'DeepSeek Chat',type:'chat',enabled:true},
      {id:'ds-reasoner',modelId:'deepseek-reasoner',name:'DeepSeek Reasoner',type:'chat',enabled:true}
    ]},
    {id:'local',name:'本地模型',kind:'custom',baseUrl:'http://localhost:8000/v1',protocol:'openai-completions',enabled:true,models:[
      {id:'qwen-vl',modelId:'Qwen3.5-VL-4B',name:'Qwen 视觉模型',type:'vision',enabled:true},
      {id:'qwen-embed',modelId:'Qwen3-Embedding-4B',name:'Qwen Embedding',type:'embedding',enabled:true}
    ]},
    {id:'custom',name:'团队模型服务',kind:'custom',baseUrl:'https://ai.example.com/v1',protocol:'openai-completions',enabled:false,models:[
      {id:'team-chat',modelId:'team-chat',name:'团队对话模型',type:'chat',enabled:false}
    ]}
  ],
  agents: [
    {id:'planner',name:'工作规划助手',description:'梳理工作思路，拆解任务与安排优先级。',icon:'list-checks',providerId:'deepseek',modelId:'ds-chat',systemPrompt:'你是一位严谨的工作规划助手。帮助用户拆解目标、识别依赖、安排优先级。区分已知信息与假设，不声称操作过未连接的系统。',temperature:0.5,maxTokens:4096,enabled:true},
    {id:'developer',name:'研发助手',description:'讨论技术方案、分析代码与定位问题。',icon:'code-2',providerId:'deepseek',modelId:'ds-reasoner',systemPrompt:'你是一位务实的软件工程助手。依据用户提供的信息分析问题，给出可验证的建议。',temperature:0.3,maxTokens:8192,enabled:true},
    {id:'writer',name:'文档助手',description:'整理会议纪要，润色周报与项目文档。',icon:'file-pen-line',providerId:'deepseek',modelId:'ds-chat',systemPrompt:'你是一位中文文档助手。保持事实准确、层次清楚、表达简洁。',temperature:0.7,maxTokens:4096,enabled:true},
    {id:'vision',name:'视觉分析助手',description:'讨论图片分析需求与视觉任务的提示词。',icon:'scan-eye',providerId:'local',modelId:'qwen-vl',systemPrompt:'你是一位视觉分析助手。只根据实际提供的图片或描述回答，不编造不可见的细节。',temperature:0.4,maxTokens:4096,enabled:false}
  ],
  conversations:[{id:'chat-demo',agentId:'planner',title:'梳理本周的工作安排',createdAt:'2026-09-07T09:00:00+08:00',messages:[
    {id:'m1',role:'user',content:'这周有需求梳理、首页设计和接口联调，帮我整理一下优先级。'},
    {id:'m2',role:'assistant',content:'可以先按依赖关系安排：\n\n1. 需求梳理：先明确范围和验收标准，避免后续返工。\n2. 首页设计：基于已确认的需求，确定页面结构和交互。\n3. 接口联调：在字段和交互达成一致后安排完整联调。\n\n今天可以先整理待确认问题，再为设计与联调各预留一段连续时间。',demo:true}
  ]}]
};
