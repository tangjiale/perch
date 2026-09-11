import { expect, it } from "vitest";
import { emptySnapshot } from "./api";
import { newChatConversation } from "./chat-conversation";
import type { Snapshot } from "./types";
const data: Snapshot = {
  ...emptySnapshot,
  agents: [
    {
      id: "a",
      name: "任务助手",
      description: "",
      enabled: true,
      modelId: "m",
      systemPrompt: "",
      temperature: 0.5,
      maxTokens: 1024,
      skillIds: [],
    },
  ],
  models: [
    {
      id: "m",
      name: "模型",
      enabled: true,
      providerId: "p",
      remoteModelId: "remote",
      capability: "vision",
    },
  ],
  providers: [
    {
      id: "p",
      name: "供应商",
      enabled: true,
      baseUrl: "https://example.test",
      protocol: "openai-completions",
    },
  ],
};
it("首次消息绑定选择的 Agent、模型与知识库", () => {
  const value = newChatConversation(data, "a", "knowledge", "new-id");
  expect(value).toMatchObject({
    id: "new-id",
    agentId: "a",
    knowledgeId: "knowledge",
    agentSnapshot: { model: { id: "m" }, provider: { id: "p" } },
  });
});
it("配置缺失或停用时给出明确指引", () => {
  expect(() => newChatConversation(data, "missing", "", "id")).toThrow("选择");
  expect(() =>
    newChatConversation({ ...data, models: [] }, "a", "", "id"),
  ).toThrow("聊天模型");
  expect(() =>
    newChatConversation({ ...data, providers: [] }, "a", "", "id"),
  ).toThrow("供应商");
  expect(() =>
    newChatConversation(
      { ...data, models: [{ ...data.models[0], capability: "embedding" }] },
      "a",
      "",
      "id",
    ),
  ).toThrow("聊天模型");
});
