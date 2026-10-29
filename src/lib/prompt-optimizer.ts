import type { Base, Message, Model, Provider } from "./types";

export type PromptKind = "system" | "user";
export type PromptStrategy = "structured" | "concise" | "detailed";
export interface PromptVersion { id: string; content: string; createdAt: number }
export interface PromptEntry extends Base {
  title: string;
  content: string;
  kind: PromptKind;
  tags: string[];
  favorite: boolean;
  versions: PromptVersion[];
}
export interface PromptRequest {
  generationId: string;
  modelId: string;
  action: "optimize" | "iterate" | "test";
  kind: PromptKind;
  strategy: PromptStrategy;
  prompt: string;
  instruction?: string;
  testInput?: string;
}
export interface PromptProgress { generationId: string; content: string }
export interface PromptResult {
  content: string;
  usage?: Message["usage"];
  durationMs: number;
  finishedAt: number;
}
export const PROMPT_LIMIT = 30_000;
export const promptKindLabels: Record<PromptKind, string> = { system: "系统提示词", user: "用户提示词" };
export const promptStrategies = [
  { value: "structured", label: "结构化", hint: "明确角色、目标、约束与输出格式" },
  { value: "concise", label: "精简表达", hint: "删去冗余，保留必要信息与约束" },
  { value: "detailed", label: "细化要求", hint: "补全任务步骤、边界与验收标准" },
] as const;

export function promptModels(models: Model[], providers: Provider[]) {
  return models.filter((model) => model.enabled && (model.capability === "chat" || model.capability === "vision") &&
    providers.some((provider) => provider.id === model.providerId && provider.enabled));
}

export function filterPrompts(entries: PromptEntry[], query: string, tag: string, favoritesOnly: boolean) {
  const needle = query.trim().toLocaleLowerCase();
  return entries.filter((entry) => (!favoritesOnly || entry.favorite) && (!tag || entry.tags.includes(tag)) &&
    (!needle || [entry.title, entry.content, ...entry.tags].some((value) => value.toLocaleLowerCase().includes(needle))))
    .sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0) || a.id.localeCompare(b.id));
}

export function parsePromptTags(value: string) {
  return [...new Set(value.split(/[,，]/).map((tag) => tag.trim()).filter(Boolean))];
}
