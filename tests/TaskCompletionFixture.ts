/** 完成回顾隔离验收：仅使用内存示例，禁止业务写入与外部请求。 */
import { DateTime, Settings } from "luxon";
import { emptySnapshot } from "../src/lib/api";
import type { Project, Task } from "../src/lib/types";

Settings.defaultZone = "Asia/Shanghai";
const fixtureNow = DateTime.fromISO("2026-10-16T16:00:00+08:00").toMillis();
Settings.now = () => fixtureNow;
const projects: Project[] = [
  { id: "p1", name: "栖点体验优化", source: "local", status: "doing", owner: "", description: "" },
  { id: "p2", name: "个人学习计划", source: "local", status: "doing", owner: "", description: "" },
];
function task(id: string, title: string, date: string | null, projectId = "p1"): Task {
  return {
    id, title, projectId, revision: 1, notes: "<p>完成回顾隔离示例</p>",
    source: "local", status: "done", priority: "normal", sortOrder: 0,
    completedAt: date ? DateTime.fromISO(date).toMillis() : null,
  };
}
const tasks: Task[] = [
  task("this-1", "完善任务完成回顾与项目筛选", "2026-10-16T15:20:00"),
  { ...task("this-2", "优化日历排期的跨日显示", "2026-10-15T17:40:00"), source: "zentao", completedAtSource: "remote", remoteId: "498", remoteType: "execution", remoteStatus: "closed", connectionId: "demo" },
  task("this-3", "整理本周阅读笔记", "2026-10-14T20:30:00", "p2"),
  task("this-4", "核对任务备注中较长的业务名称、跨项目协作信息以及桌面窄窗口下的换行展示", "2026-10-12T09:10:00"),
  task("last-1", "完成邮件阅读体验优化", "2026-10-11T23:59:59"),
  task("last-2", "更新项目说明与使用文档", "2026-10-09T16:00:00"),
  task("last-3", "整理个人学习资料", "2026-10-05T00:00:00", "p2"),
  task("unknown", "归档历史执行记录", null),
  task("older", "更早完成的任务不进入两周回顾", "2026-10-04T23:59:59"),
  task("future", "未来完成时间不进入回顾", "2026-10-17T09:00:00"),
  { ...task("reopened", "重新打开的任务不进入回顾", "2026-10-15T09:00:00"), status: "doing" },
];
const snapshot = { ...emptySnapshot, projects, tasks, connections: [{
  id: "demo", name: "演示禅道", baseUrl: "https://zentao.example.test", apiVersion: "v1",
  enabled: true, managementEnabled: true, hasCredential: false,
}] };
const internal = (window as unknown as {
  __TAURI_INTERNALS__: { invoke: (name: string) => Promise<unknown> };
}).__TAURI_INTERNALS__;
internal.invoke = async (name) => {
  if (name === "workspace_snapshot") return structuredClone(snapshot);
  if (name === "storage_info") return { dataRoot: "/demo/perch", defaultRoot: "/demo/perch", workspaceId: "completion-fixture", version: "0.1.19" };
  if (name === "mail_unread") return { total: 0, accounts: [] };
  if (name === "task_assignment") return { assignedBy: null, assignedAt: null, createdBy: null };
  if (name === "plugin:event|listen") return 1;
  if (name.startsWith("plugin:window|") || name === "plugin:event|unlisten") return null;
  throw Error(`隔离验收禁止命令：${name}`);
};
localStorage.setItem("perch.appearance.v1", JSON.stringify({ mode: "light", chatFontSize: 14 }));
window.location.hash = "/tasks";
await import("../src/main");
