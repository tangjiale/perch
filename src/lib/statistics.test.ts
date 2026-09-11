import { describe, expect, it } from "vitest";
import { buildStatistics, type Count } from "./statistics";
import type { Agent, Document, Snapshot, Task } from "./types";

const timestamp = (value: string) => Date.parse(value);
const now = timestamp("2026-09-08T12:00:00+08:00");
const empty = (): Snapshot => ({
  tasks: [],
  projects: [],
  apps: [],
  categories: [],
  providers: [],
  models: [],
  agents: [],
  skills: [],
  knowledge: [],
  documents: [],
  conversations: [],
  messages: [],
  connections: [],
});
const task = (id: string, overrides: Partial<Task> = {}): Task => ({
  id,
  title: id,
  notes: "",
  source: "local",
  status: "todo",
  priority: "normal",
  sortOrder: 0,
  ...overrides,
});
const agent = (id: string, overrides: Partial<Agent> = {}): Agent => ({
  id,
  name: id,
  description: "",
  systemPrompt: "",
  modelId: "m",
  temperature: 1,
  maxTokens: 2048,
  enabled: true,
  skillIds: [],
  ...overrides,
});
const document = (id: string, overrides: Partial<Document> = {}): Document => ({
  id,
  name: `${id}.pdf`,
  knowledgeId: "k",
  status: "parsed",
  size: 100,
  ...overrides,
});
const total = (values: Count[]) =>
  values.reduce((sum, value) => sum + value.value, 0);

describe("统计聚合", () => {
  it("空数据保留日期和固定状态，不伪造完成率", () => {
    const result = buildStatistics(empty(), 7, now, "Asia/Shanghai");
    expect(result.taskTrend.map((day) => day.date)).toEqual([
      "2026-09-02",
      "2026-09-03",
      "2026-09-04",
      "2026-09-05",
      "2026-09-06",
      "2026-09-07",
      "2026-09-08",
    ]);
    expect(
      result.chatTrend.every(
        (day) => day.sessions + day.userMessages + day.assistantMessages === 0,
      ),
    ).toBe(true);
    expect(result.taskStatus.map((item) => item.name)).toEqual([
      "待做",
      "正在做",
      "已完成",
      "已关闭",
    ]);
    expect(result.taskMetrics.completionRate).toBeNull();
    expect(Object.values(result.metrics).every((value) => value === 0)).toBe(
      true,
    );
    expect(result.libraryUsage).toEqual([]);
  });

  it("任务按当前状态统计，仅有完成时间且仍完成的任务进入完成趋势", () => {
    const data = empty();
    const createdAt = timestamp("2026-09-07T23:59:00+08:00");
    const completedAt = timestamp("2026-09-08T00:01:00+08:00");
    data.tasks = [
      task("done", {
        status: "done",
        createdAt,
        completedAt,
        schedule: {
          kind: "all_day",
          timezone: "Asia/Shanghai",
          start: "2026-09-01",
          end: "2026-09-02",
        },
      }),
      task("legacy", { status: "done", completedAt: null }),
      task("reopened", {
        status: "doing",
        completedAt,
        schedule: {
          kind: "all_day",
          start: "2026-09-07",
          end: "2026-09-08",
          timezone: "Asia/Shanghai",
        },
      }),
      task("closed", {
        status: "closed",
        completedAt,
        schedule: {
          kind: "all_day",
          timezone: "Asia/Shanghai",
          start: "2026-09-01",
          end: "2026-09-02",
        },
      }),
      task("today", {
        schedule: {
          kind: "all_day",
          timezone: "Asia/Shanghai",
          start: "2026-09-08",
          end: "2026-09-09",
        },
      }),
      task("bad", { createdAt: Number.NaN }),
      task("bad-format", { createdAt: Infinity }),
    ];
    const result = buildStatistics(data, 7, now, "Asia/Shanghai");
    expect(result.taskMetrics).toEqual({
      total: 7,
      open: 4,
      done: 2,
      closed: 1,
      overdue: 2,
      unscheduled: 3,
      completionRate: 2 / 6,
    });
    expect(result.taskTrend[5]).toEqual({
      date: "2026-09-07",
      created: 1,
      completed: 0,
    });
    expect(result.taskTrend[6]).toEqual({
      date: "2026-09-08",
      created: 0,
      completed: 1,
    });
    expect(result.projectProgress[0]).toMatchObject({
      name: "未关联项目",
      todo: 3,
      doing: 1,
      done: 2,
      closed: 1,
      total: 7,
    });
    expect(
      buildStatistics(
        { ...data, tasks: [task("closed", { status: "closed" })] },
        7,
        now,
      ).taskMetrics.completionRate,
    ).toBeNull();
  });

  it.each([
    [
      "2026-03-10T00:30:00-04:00",
      "2026-03-08T01:59:00-05:00",
      "2026-03-08T03:01:00-04:00",
      "2026-03-08",
    ],
    [
      "2026-11-03T00:30:00-05:00",
      "2026-11-01T01:30:00-04:00",
      "2026-11-01T01:30:00-05:00",
      "2026-11-01",
    ],
  ])(
    "夏令时自然日连续且重复小时只计真实记录 %s",
    (current, first, second, date) => {
      const data = empty();
      data.tasks = [
        task("a", { createdAt: timestamp(first) }),
        task("b", { createdAt: timestamp(second) }),
      ];
      data.conversations = [
        { id: "c", title: "c", agentId: "gone", createdAt: timestamp(first) },
      ];
      data.messages = [
        {
          id: "m",
          conversationId: "c",
          role: "user",
          content: "",
          status: "completed",
          createdAt: timestamp(second),
        },
      ];
      const result = buildStatistics(
        data,
        7,
        timestamp(current),
        "America/New_York",
      );
      expect(new Set(result.taskTrend.map((day) => day.date)).size).toBe(7);
      expect(result.taskTrend.find((day) => day.date === date)?.created).toBe(
        2,
      );
      expect(result.chatTrend.find((day) => day.date === date)).toEqual({
        date,
        sessions: 1,
        userMessages: 1,
        assistantMessages: 0,
      });
    },
  );

  it("窗口边界包含本地首日零点、排除前日，支持 30 和 90 天", () => {
    const data = empty();
    data.tasks = [
      task("outside", { createdAt: timestamp("2026-09-01T23:59:59+08:00") }),
      task("inside", { createdAt: timestamp("2026-09-02T00:00:00+08:00") }),
    ];
    expect(
      buildStatistics(data, 7, now, "Asia/Shanghai").taskTrend[0].created,
    ).toBe(1);
    expect(
      buildStatistics(data, 30, now, "Asia/Shanghai").taskTrend,
    ).toHaveLength(30);
    expect(
      buildStatistics(data, 90, now, "Asia/Shanghai").chatTrend,
    ).toHaveLength(90);
  });

  it("保留空项目和空知识库，孤儿引用、超长名称与特殊名称不丢数", () => {
    const data = empty();
    const longName = "<script>名称</script>".repeat(100);
    data.projects = [
      {
        id: "p",
        name: longName,
        description: "",
        source: "local",
        status: "todo",
        owner: "",
      },
    ];
    data.tasks = [task("t", { projectId: "deleted" })];
    data.categories = [{ id: "c", name: "__proto__" }];
    data.apps = ["c", "deleted", ""].map((categoryId, i) => ({
      id: `app${i}`,
      name: longName,
      url: "",
      description: "",
      favorite: i === 0,
      sortOrder: i,
      categoryId,
    }));
    data.knowledge = [
      { id: "k", name: longName, description: "", modelId: "m" },
    ];
    data.documents = [
      document("orphan", {
        knowledgeId: "deleted",
        chunks: 3,
        status: "ready",
      }),
    ];
    data.agents = [agent("a")];
    data.conversations = [
      { id: "c1", title: "", agentId: "a" },
      {
        id: "c2",
        title: "",
        agentId: "gone",
        agentSnapshot: agent("gone", { name: longName }),
        knowledgeId: "gone",
      },
      { id: "c3", title: "", agentId: "unknown" },
    ];
    const result = buildStatistics(data, 7, now, "Asia/Shanghai");
    expect(result.projectProgress).toEqual([
      {
        id: "__unassigned__",
        name: "未关联项目",
        todo: 1,
        doing: 0,
        done: 0,
        closed: 0,
        total: 1,
      },
      {
        id: "p",
        name: longName,
        todo: 0,
        doing: 0,
        done: 0,
        closed: 0,
        total: 0,
      },
    ]);
    expect(
      result.libraryUsage.find((library) => library.id === "k"),
    ).toMatchObject({ name: longName, documents: 0 });
    expect(result.libraryUsage[0]).toMatchObject({
      name: "未关联知识库",
      documents: 1,
      bytes: 100,
      chunks: 3,
      ready: 1,
    });
    expect(result.appCategories).toEqual([
      { name: "__proto__", value: 1 },
      { name: "分类已删除", value: 1 },
      { name: "未分类", value: 1 },
    ]);
    expect(result.agentUsage).toContainEqual({
      name: `${longName}（已删除）`,
      value: 1,
    });
    expect(total(result.agentUsage)).toBe(3);
    expect(result.metrics.ragConversations).toBe(1);
  });

  it("同名分类、供应商与智能体按实体 ID 独立计数，唯一名称和孤儿组保持稳定", () => {
    const data = empty();
    data.categories = [
      { id: "c1", name: "研发" },
      { id: "c2", name: "研发" },
      { id: "c3", name: "运营" },
    ];
    data.apps = ["c1", "c2", "c2", "c3", "gone", ""].map((categoryId, i) => ({
      id: `app${i}`,
      name: "应用",
      url: "",
      description: "",
      favorite: false,
      sortOrder: i,
      categoryId,
    }));
    data.providers = ["p1", "p2", "p3"].map((id) => ({
      id,
      name: id === "p3" ? "独立供应商" : "同名供应商",
      baseUrl: "",
      protocol: "openai-completions",
      enabled: true,
    }));
    data.models = ["p1", "p2", "p2", "p3", "gone"].map((providerId, i) => ({
      id: `m${i}`,
      name: "模型",
      remoteModelId: "m",
      providerId,
      capability: "chat",
      enabled: true,
    }));
    data.agents = [
      agent("a1", { name: "助手" }),
      agent("a2", { name: "助手" }),
      agent("a3", { name: "独立助手" }),
    ];
    data.conversations = ["a1", "a2", "a2", "a3", "d1", "d2", "d2", "gone"].map(
      (agentId, i) => ({
        id: `chat${i}`,
        title: "",
        agentId,
        ...(agentId.startsWith("d")
          ? { agentSnapshot: agent(agentId, { name: "旧助手" }) }
          : {}),
      }),
    );
    const result = buildStatistics(data, 7, now);
    expect(result.appCategories).toEqual([
      { name: "研发（c1）", value: 1 },
      { name: "研发（c2）", value: 2 },
      { name: "运营", value: 1 },
      { name: "分类已删除", value: 1 },
      { name: "未分类", value: 1 },
    ]);
    expect(result.providerModels).toEqual([
      { name: "同名供应商（p1）", value: 1 },
      { name: "同名供应商（p2）", value: 2 },
      { name: "独立供应商", value: 1 },
      { name: "未关联供应商", value: 1 },
    ]);
    expect(result.agentUsage).toEqual([
      { name: "助手（a1）", value: 1 },
      { name: "助手（a2）", value: 2 },
      { name: "独立助手", value: 1 },
      { name: "旧助手（已删除）（d1）", value: 1 },
      { name: "旧助手（已删除）（d2）", value: 2 },
      { name: "未关联智能体", value: 1 },
    ]);
    expect(total(result.appCategories)).toBe(data.apps.length);
    expect(total(result.providerModels)).toBe(data.models.length);
    expect(total(result.agentUsage)).toBe(data.conversations.length);
  });

  it("文档异常大小与非数字分块不进入容量，未知状态安全归组", () => {
    const data = empty();
    data.documents = [
      document("ok", {
        size: 512,
        chunks: 4,
        status: "ready",
        name: "REPORT.PDF",
      }),
      document("infinity", {
        size: Infinity,
        chunks: Infinity,
        name: "文件.docx",
      }),
      document("negative", {
        size: -1,
        chunks: -2,
        status: "__proto__",
        name: "README",
      }),
      document("nan", {
        size: NaN,
        chunks: "8" as unknown as number,
        status: "needs_ocr",
      }),
      document("string", {
        size: "100" as unknown as number,
        name: `x.${"z".repeat(1000)}`,
      }),
    ];
    const result = buildStatistics(data, 7, now);
    expect(result.metrics).toMatchObject({
      documents: 5,
      documentBytes: 512,
      chunks: 4,
      readyDocuments: 1,
    });
    expect(result.documentStatus).toContainEqual({
      name: "未知状态",
      value: 1,
    });
    expect(total(result.documentFormats)).toBe(5);
    expect(result.documentFormats).toContainEqual({
      name: "其他格式",
      value: 2,
    });
  });

  it("回复只按 assistant 状态统计，技能绑定去重且排除已删除技能", () => {
    const data = empty();
    data.messages = [
      "completed",
      "streaming",
      "stopped",
      "failed",
      "error",
      "interrupted",
      "unknown",
    ].map((status, i) => ({
      id: `m${i}`,
      conversationId: "gone",
      role: "assistant",
      content: "",
      status,
      createdAt: now,
    }));
    data.messages.push({
      id: "user",
      conversationId: "gone",
      role: "user",
      content: "",
      status: "error",
      createdAt: now,
    });
    data.skills = ["s1", "s2", "s3"].map((id) => ({
      id,
      name: id,
      description: "",
      content: "",
      enabled: id !== "s2",
    }));
    data.agents = [
      agent("a1", { skillIds: ["s1", "s1", "gone"] }),
      agent("a2", { enabled: false, skillIds: ["s1", "s2"] }),
    ];
    data.providers = [
      {
        id: "p",
        name: "供应商",
        baseUrl: "",
        protocol: "openai-completions",
        enabled: true,
      },
    ];
    data.models = ["chat", "vision", "embedding"].map((capability, i) => ({
      id: `model${i}`,
      providerId: i === 2 ? "gone" : "p",
      name: "model",
      remoteModelId: "m",
      capability: capability as "chat" | "vision" | "embedding",
      enabled: i !== 2,
    }));
    data.tasks = [
      task("a"),
      task("b", { source: "zentao", priority: "high", status: "doing" }),
    ];
    const result = buildStatistics(data, 7, now, "Asia/Shanghai");
    expect(result.metrics).toMatchObject({
      messages: 8,
      userMessages: 1,
      assistantMessages: 7,
      failedMessages: 3,
      boundSkills: 2,
      enabledSkills: 2,
      agents: 2,
      enabledAgents: 1,
      models: 3,
      enabledModels: 2,
      enabledProviders: 1,
    });
    expect(total(result.messageStatus)).toBe(result.metrics.assistantMessages);
    expect(result.chatTrend[6]).toMatchObject({
      userMessages: 1,
      assistantMessages: 7,
    });
    expect(result.providerModels).toEqual([
      { name: "供应商", value: 2 },
      { name: "未关联供应商", value: 1 },
    ]);
    for (const group of [
      result.taskStatus,
      result.taskPriority,
      result.taskSource,
    ])
      expect(total(group)).toBe(data.tasks.length);
    for (const group of [result.modelCapabilities, result.providerModels])
      expect(total(group)).toBe(data.models.length);
    expect(total(result.appCategories)).toBe(data.apps.length);
    expect(total(result.documentStatus)).toBe(data.documents.length);
    expect(total(result.documentFormats)).toBe(data.documents.length);
    expect(
      result.projectProgress.reduce((sum, project) => sum + project.total, 0),
    ).toBe(data.tasks.length);
  });
});
