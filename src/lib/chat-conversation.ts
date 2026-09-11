import type { Conversation, Snapshot } from "./types";

export function newChatConversation(
  data: Snapshot,
  agentId: string,
  knowledgeId: string,
  id: string,
): Conversation {
  const agent = data.agents.find((item) => item.id === agentId && item.enabled);
  if (!agent) throw new Error("请先选择可用的 Agent");
  const model = data.models.find(
    (item) =>
      item.id === agent.modelId &&
      item.enabled &&
      item.capability !== "embedding",
  );
  if (!model)
    throw new Error(
      "该 Agent 尚未配置可用的聊天模型，请到「设置 → Agent」配置模型后再发送。",
    );
  const provider = data.providers.find(
    (item) => item.id === model.providerId && item.enabled,
  );
  if (!provider)
    throw new Error("该 Agent 的模型供应商不可用，请到设置中检查供应商配置。");
  return {
    id,
    title: "新会话",
    agentId: agent.id,
    knowledgeId: knowledgeId || undefined,
    agentSnapshot: {
      ...agent,
      model,
      provider,
      skills: data.skills.filter((skill) => agent.skillIds.includes(skill.id)),
    },
  };
}
