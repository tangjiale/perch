import { DateTime } from "luxon";
import { statuses, type Snapshot } from "./types";
import { isTaskExpired } from "./task-expiry";

export type Count = { name: string; value: number };

function counts(names: string[] = []) {
  const values = new Map(names.map((name) => [name, 0]));
  return {
    add(name: string) {
      values.set(name, (values.get(name) ?? 0) + 1);
    },
    result(): Count[] {
      return Array.from(values, ([name, value]) => ({ name, value }));
    },
  };
}

function entityCounts(entities: { id: string; name: string }[]) {
  const values = new Map<string | symbol, Count>(
    entities.map(({ id, name }) => [id, { name, value: 0 }]),
  );
  return {
    add(id: string | symbol, name: string) {
      const existing = values.get(id);
      if (existing) existing.value += 1;
      else values.set(id, { name, value: 1 });
    },
    result(): Count[] {
      const frequencies = new Map<string, number>();
      for (const { name } of values.values())
        frequencies.set(name, (frequencies.get(name) ?? 0) + 1);
      const usedNames = new Set(frequencies.keys());
      return Array.from(values, ([id, { name, value }]) => {
        if (frequencies.get(name) === 1) return { name, value };
        const suffix = typeof id === "string" ? id : "汇总分组";
        let distinctName = `${name}（${suffix}）`;
        let serial = 2;
        while (usedNames.has(distinctName))
          distinctName = `${name}（${suffix} · ${serial++}）`;
        usedNames.add(distinctName);
        return { name: distinctName, value };
      });
    },
  };
}

const nonNegative = (value: unknown): number =>
  typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : 0;

const documentStates: Record<string, string> = {
  ready: "已索引",
  parsed: "待索引",
  needs_ocr: "待识别",
  failed: "处理失败",
  error: "处理失败",
  pending: "待处理",
  queued: "待处理",
  processing: "处理中",
  parsing: "处理中",
  indexing: "处理中",
};
const replyStates: Record<string, string> = {
  completed: "已完成",
  streaming: "生成中",
  stopped: "已停止",
  failed: "生成失败",
  error: "生成失败",
  interrupted: "已中断",
};
const capabilities: Record<string, string> = {
  chat: "对话",
  vision: "视觉",
  embedding: "向量嵌入",
};
const label = (
  labels: Record<string, string>,
  key: string,
  fallback: string,
) => (Object.hasOwn(labels, key) ? labels[key] : fallback);

export function buildStatistics(
  data: Snapshot,
  days: 7 | 30 | 90,
  now: number = Date.now(),
  zone?: string,
) {
  const today = DateTime.fromMillis(now, { zone }).startOf("day");
  // 按自然日递减，避免夏令时切换时用固定 24 小时造成错日。
  const dates = Array.from(
    { length: days },
    (_, index) => today.minus({ days: days - index - 1 }).toISODate() ?? "",
  );
  const taskTrend = dates.map((date) => ({ date, created: 0, completed: 0 }));
  const chatTrend = dates.map((date) => ({
    date,
    sessions: 0,
    userMessages: 0,
    assistantMessages: 0,
  }));
  const dateIndexes = new Map(dates.map((date, index) => [date, index]));
  const indexFor = (timestamp: number | null | undefined) => {
    if (typeof timestamp !== "number" || !Number.isFinite(timestamp))
      return undefined;
    const date = DateTime.fromMillis(timestamp, { zone }).toISODate();
    return date ? dateIndexes.get(date) : undefined;
  };
  const taskStatus = counts(Object.values(statuses));
  const taskPriority = counts(["高", "普通", "低"]);
  const taskSource = counts(["本地", "禅道"]);
  const projects = new Map(
    data.projects.map((project) => [
      project.id,
      {
        id: project.id,
        name: project.name,
        todo: 0,
        doing: 0,
        done: 0,
        closed: 0,
        total: 0,
      },
    ]),
  );
  const unassigned = {
    id: "__unassigned__",
    name: "未关联项目",
    todo: 0,
    doing: 0,
    done: 0,
    closed: 0,
    total: 0,
  };
  const taskMetrics = {
    total: data.tasks.length,
    open: 0,
    done: 0,
    closed: 0,
    overdue: 0,
    unscheduled: 0,
    completionRate: null as number | null,
  };
  for (const task of data.tasks) {
    taskStatus.add(label(statuses, task.status, "未知状态"));
    taskPriority.add(
      label(
        { high: "高", normal: "普通", low: "低" },
        task.priority,
        "未知优先级",
      ),
    );
    taskSource.add(
      label({ local: "本地", zentao: "禅道" }, task.source, "未知来源"),
    );
    const project =
      (task.projectId && projects.get(task.projectId)) || unassigned;
    if (Object.hasOwn(statuses, task.status)) project[task.status] += 1;
    project.total += 1;
    const open = task.status === "todo" || task.status === "doing";
    if (open) taskMetrics.open += 1;
    if (task.status === "done") taskMetrics.done += 1;
    if (task.status === "closed") taskMetrics.closed += 1;
    if (!task.schedule?.start) taskMetrics.unscheduled += 1;
    if (isTaskExpired(task, DateTime.fromMillis(now, { zone }))) {
      taskMetrics.overdue += 1;
    }
    const createdIndex = indexFor(task.createdAt);
    if (createdIndex !== undefined) taskTrend[createdIndex].created += 1;
    const completedIndex =
      task.status === "done" ? indexFor(task.completedAt) : undefined;
    if (completedIndex !== undefined) taskTrend[completedIndex].completed += 1;
  }
  const activeTotal = taskMetrics.total - taskMetrics.closed;
  taskMetrics.completionRate = activeTotal
    ? taskMetrics.done / activeTotal
    : null;
  const projectProgress = [
    ...projects.values(),
    ...(unassigned.total ? [unassigned] : []),
  ].sort((a, b) => b.total - a.total || a.name.localeCompare(b.name, "zh-CN"));

  const categoryNames = new Map(
    data.categories.map((category) => [category.id, category.name]),
  );
  const appCategories = entityCounts(data.categories);
  const deletedCategory = Symbol("deletedCategory");
  const uncategorized = Symbol("uncategorized");
  for (const app of data.apps) {
    const name = app.categoryId ? categoryNames.get(app.categoryId) : undefined;
    if (name !== undefined) appCategories.add(app.categoryId!, name);
    else
      appCategories.add(
        app.categoryId ? deletedCategory : uncategorized,
        app.categoryId ? "分类已删除" : "未分类",
      );
  }

  const libraries = new Map(
    data.knowledge.map((library) => [
      library.id,
      {
        id: library.id,
        name: library.name,
        documents: 0,
        bytes: 0,
        ready: 0,
        chunks: 0,
      },
    ]),
  );
  const orphanLibrary = {
    id: "__unassigned__",
    name: "未关联知识库",
    documents: 0,
    bytes: 0,
    ready: 0,
    chunks: 0,
  };
  const documentStatus = counts([
    "已索引",
    "待索引",
    "待识别",
    "处理失败",
    "待处理",
    "处理中",
  ]);
  const documentFormats = counts();
  for (const document of data.documents) {
    const library = libraries.get(document.knowledgeId) ?? orphanLibrary;
    library.documents += 1;
    library.bytes += nonNegative(document.size);
    library.chunks += nonNegative(document.chunks);
    if (document.status === "ready") library.ready += 1;
    documentStatus.add(label(documentStates, document.status, "未知状态"));
    const extension = document.name.match(/\.([a-zA-Z0-9]{1,12})$/)?.[1];
    documentFormats.add(extension ? extension.toUpperCase() : "其他格式");
  }
  const libraryUsage = [
    ...libraries.values(),
    ...(orphanLibrary.documents ? [orphanLibrary] : []),
  ].sort(
    (a, b) =>
      b.documents - a.documents || a.name.localeCompare(b.name, "zh-CN"),
  );

  const agentNames = new Map(
    data.agents.map((agent) => [agent.id, agent.name]),
  );
  for (const conversation of data.conversations) {
    if (
      !agentNames.has(conversation.agentId) &&
      conversation.agentSnapshot?.name
    ) {
      agentNames.set(
        conversation.agentId,
        `${conversation.agentSnapshot.name}（已删除）`,
      );
    }
  }
  const agentUsage = entityCounts(data.agents);
  const orphanAgent = Symbol("orphanAgent");
  for (const conversation of data.conversations) {
    const name = agentNames.get(conversation.agentId);
    agentUsage.add(
      name !== undefined ? conversation.agentId : orphanAgent,
      name ?? "未关联智能体",
    );
    const index = indexFor(conversation.createdAt);
    if (index !== undefined) chatTrend[index].sessions += 1;
  }
  const messageStatus = counts([
    "已完成",
    "生成中",
    "已停止",
    "生成失败",
    "已中断",
  ]);
  let userMessages = 0;
  let assistantMessages = 0;
  let failedMessages = 0;
  for (const message of data.messages) {
    const index = indexFor(message.createdAt);
    if (message.role === "user") {
      userMessages += 1;
      if (index !== undefined) chatTrend[index].userMessages += 1;
    } else if (message.role === "assistant") {
      assistantMessages += 1;
      messageStatus.add(label(replyStates, message.status, "未知状态"));
      if (["error", "failed", "interrupted"].includes(message.status))
        failedMessages += 1;
      if (index !== undefined) chatTrend[index].assistantMessages += 1;
    }
  }
  const providerNames = new Map(
    data.providers.map((provider) => [provider.id, provider.name]),
  );
  const providerModels = entityCounts(data.providers);
  const orphanProvider = Symbol("orphanProvider");
  const modelCapabilities = counts(Object.values(capabilities));
  for (const model of data.models) {
    const name = providerNames.get(model.providerId);
    providerModels.add(
      name !== undefined ? model.providerId : orphanProvider,
      name ?? "未关联供应商",
    );
    modelCapabilities.add(label(capabilities, model.capability, "未知能力"));
  }
  const boundSkillIds = new Set(data.agents.flatMap((agent) => agent.skillIds));
  const metrics = {
    projects: data.projects.length,
    apps: data.apps.length,
    favorites: data.apps.filter((app) => app.favorite).length,
    knowledge: data.knowledge.length,
    documents: data.documents.length,
    documentBytes: libraryUsage.reduce(
      (sum, library) => sum + library.bytes,
      0,
    ),
    readyDocuments: libraryUsage.reduce(
      (sum, library) => sum + library.ready,
      0,
    ),
    chunks: libraryUsage.reduce((sum, library) => sum + library.chunks, 0),
    conversations: data.conversations.length,
    messages: data.messages.length,
    userMessages,
    assistantMessages,
    failedMessages,
    ragConversations: data.conversations.filter(
      (conversation) => !!conversation.knowledgeId,
    ).length,
    providers: data.providers.length,
    enabledProviders: data.providers.filter((provider) => provider.enabled)
      .length,
    models: data.models.length,
    enabledModels: data.models.filter((model) => model.enabled).length,
    agents: data.agents.length,
    enabledAgents: data.agents.filter((agent) => agent.enabled).length,
    skills: data.skills.length,
    enabledSkills: data.skills.filter((skill) => skill.enabled).length,
    boundSkills: data.skills.filter((skill) => boundSkillIds.has(skill.id))
      .length,
  };
  return {
    taskStatus: taskStatus.result(),
    taskPriority: taskPriority.result(),
    taskSource: taskSource.result(),
    taskTrend,
    projectProgress,
    taskMetrics,
    appCategories: appCategories.result(),
    documentStatus: documentStatus.result(),
    documentFormats: documentFormats.result(),
    agentUsage: agentUsage.result(),
    modelCapabilities: modelCapabilities.result(),
    providerModels: providerModels.result(),
    libraryUsage,
    chatTrend,
    metrics,
    messageStatus: messageStatus.result(),
  };
}
