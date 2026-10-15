import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import Chat from "../src/features/Chat";
import { emptySnapshot } from "../src/lib/api";
import type { Snapshot } from "../src/lib/types";

export function mountChat(
  host: HTMLElement,
  empty = false,
  modelSwitch = false,
) {
  const root = createRoot(host);
  const provider = {
    id: "provider",
    name: "本地测试服务",
    baseUrl: "http://localhost:9000/v1",
    protocol: "openai-completions" as const,
    enabled: true,
  };
  const models: Snapshot["models"] = ["vision", "chat"].map((capability) => ({
    id: capability,
    name: capability === "vision" ? "Qwen3.5-VL-4B" : "文本模型",
    remoteModelId: capability,
    providerId: provider.id,
    capability: capability as "vision" | "chat",
    enabled: true,
  }));
  const agents = models.map((model) => ({
    id: model.id,
    name: model.capability === "vision" ? "视觉助手" : "文本助手",
    modelId: model.id,
    systemPrompt: "",
    temperature: 0.5,
    maxTokens: 1024,
    enabled: true,
    skillIds: [],
  }));
  if (modelSwitch)
    models.push(
      { ...models[0], id: "disabled-model", name: "禁用模型", enabled: false },
      {
        ...models[0],
        id: "embedding-model",
        name: "向量模型",
        capability: "embedding",
      },
      {
        ...models[0],
        id: "disabled-provider-model",
        name: "禁用供应商模型",
        providerId: "disabled-provider",
      },
    );
  const initial: Snapshot = {
    ...emptySnapshot,
    providers: [
      provider,
      ...(modelSwitch
        ? [{ ...provider, id: "disabled-provider", enabled: false }]
        : []),
    ],
    models,
    agents,
    conversations: empty
      ? []
      : agents.map((agent, i) => ({
          id: agent.id,
          title: agent.name,
          agentId: agent.id,
          revision: 1,
          agentSnapshot: { ...agent, model: models[i], provider },
          createdAt: i,
        })),
  };
  if (modelSwitch) Object.assign(window, { __chatStore: initial });
  function Fixture() {
    const [data, setData] = useState(initial);
    Object.assign(window, {
      updateChatFixture: (messages: Snapshot["messages"]) =>
        setData((d) => ({ ...d, messages })),
    });
    return (
      <MemoryRouter
        initialEntries={[empty ? "/chat" : "/chat?conversation=vision"]}
      >
        <Chat
          data={data}
          refresh={async () => {
            if (
              modelSwitch &&
              (window as unknown as { __refreshFail?: boolean }).__refreshFail
            )
              throw new Error("模拟刷新失败");
            if (
              modelSwitch &&
              !(window as unknown as { __staleChatRefresh?: boolean })
                .__staleChatRefresh
            )
              setData({
                ...(window as unknown as { __chatStore: Snapshot }).__chatStore,
              });
          }}
          notify={(message) => {
            Object.assign(window, { lastNotice: message });
          }}
        />
      </MemoryRouter>
    );
  }
  root.render(<Fixture />);
  return () => root.unmount();
}
