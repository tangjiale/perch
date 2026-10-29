import { describe, expect, it } from "vitest";
import { filterPrompts, parsePromptTags, promptModels, type PromptEntry } from "./prompt-optimizer";
import type { Model, Provider } from "./types";

it("提示词生成只允许可用文本和视觉模型，语音与向量模型不混入", () => {
  const models = (["chat", "vision", "embedding", "speech"] as const).map((capability): Model => ({ id: capability, name: capability, providerId: "p", remoteModelId: capability, capability, enabled: true }));
  const provider: Provider = { id: "p", name: "服务", baseUrl: "https://example.test/v1", protocol: "openai-completions", enabled: true };
  expect(promptModels(models, [provider]).map((model) => model.id)).toEqual(["chat", "vision"]);
  expect(promptModels(models, [{ ...provider, enabled: false }])).toEqual([]);
});

function entry(id: string, overrides: Partial<PromptEntry> = {}): PromptEntry {
  return { id, title: "日常写作", content: "保留 {{topic}} 并给出建议", kind: "system", tags: ["写作"], favorite: false, versions: [], updatedAt: 1, ...overrides };
}

describe("个人提示词筛选", () => {
  it("按标题、正文及标签搜索，并组合收藏与标签条件", () => {
    const entries = [entry("a", { title: "CODE Review", tags: ["开发"], favorite: true }), entry("b", { tags: ["开发"], favorite: false }), entry("c", { tags: ["写作"], favorite: true })];
    expect(filterPrompts(entries, " code ", "开发", true).map((item) => item.id)).toEqual(["a"]);
    expect(filterPrompts(entries, "{{topic}}", "开发", true).map((item) => item.id)).toEqual(["a"]);
    expect(filterPrompts(entries, "写作", "", true).map((item) => item.id)).toEqual(["c"]);
    expect(filterPrompts(entries, "code", "写作", false)).toEqual([]);
  });
  it("最近更新优先，同值按 ID 升序且不改动输入数组", () => {
    const entries = [entry("z"), entry("c", { updatedAt: 5 }), entry("a")];
    expect(filterPrompts(entries, "", "", false).map((item) => item.id)).toEqual(["c", "a", "z"]);
    expect(entries.map((item) => item.id)).toEqual(["z", "c", "a"]);
  });
  it("支持空列表及未设置更新时间的记录", () => {
    expect(filterPrompts([], "", "", false)).toEqual([]);
    expect(filterPrompts([entry("b", { updatedAt: undefined }), entry("a")], "", "", false).map((item) => item.id)).toEqual(["a", "b"]);
  });
});

describe("提示词标签解析", () => {
  it("兼容中英文逗号，去除空白、空标签和重复项", () => {
    expect(parsePromptTags(" 写作,开发，写作 , ， 工作 ")).toEqual(["写作", "开发", "工作"]);
    expect(parsePromptTags(" , ， ")).toEqual([]);
  });
});
