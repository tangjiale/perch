import React from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter, useLocation } from "react-router-dom";
import { Projects } from "../src/features/Work";
import Bugs from "../src/features/Bugs";
import { emptySnapshot } from "../src/lib/api";
import type { Project, Status } from "../src/lib/types";
import "../src/styles.css";
import "../src/glass.css";
import "../src/theme.css";
const projects: Project[] = [
  "客户服务平台",
  "内部工具升级",
  "推理引擎与跨平台兼容优化项目",
  "专家注册",
  "团队建设",
  "个人计划",
].map((name, index) => ({
  id: `p${index}`,
  connectionId: "test",
  revision: 1,
  name,
  source: index === 5 ? "local" : "zentao",
  remoteId: String(index + 1),
  status: "todo",
  owner: "测试负责人",
  description:
    index === 2
      ? "<p>兼容 Linux &amp; macOS</p><ol><li>完善部署流程</li></ol><img src='https://invalid.example/track' onerror='window.__projectUnsafe=true'/><script>window.__projectUnsafe=true</script>"
      : index === 3
        ? "用于测试较长项目描述的布局。".repeat(16)
        : "",
  plannedStartDate: index === 5 ? undefined : "2026-07-01",
  plannedEndDate: index === 4 || index === 5 ? undefined : "2026-09-30",
  dueDate: index === 5 ? "2026-10-12" : undefined,
}));
const data = {
  ...emptySnapshot,
  projects,
  connections: [
    {
      id: "test",
      name: "测试禅道",
      baseUrl: "https://invalid.example",
      enabled: true,
      managementEnabled: true,
      apiVersion: "v1" as const,
    },
  ],
  bugs: (["active", "active", "resolved", "closed"] as const).map(
    (status, index) => ({
      id: `bug-${index}`,
      remoteId: String(index + 1),
      connectionId: "test",
      projectId: "2",
      projectName: "内部工具升级",
      title: `验收BUG ${index + 1}`,
      steps: "",
      assignedTo: "me",
      severity: index + 1,
      priority: 2,
      status,
    }),
  ),
  tasks: (["todo", "doing", "doing", "done", "closed"] as Status[]).map(
    (status, index) => ({
      id: `t${index}`,
      title: "测试任务",
      notes: "",
      source: "local" as const,
      status,
      priority: "normal" as const,
      sortOrder: index,
      projectId: "p1",
    }),
  ),
};
function Fixture({ search }: { search: string }) {
  const location = useLocation();
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">栖点 · 界面验收</div>
      </aside>
      <div className="workspace">
        <main>
          <output hidden id="test-route">
            {location.pathname + location.search}
          </output>
          {location.pathname === "/bugs" ? (
            <Bugs data={data} refresh={async () => {}} notify={() => {}} />
          ) : (
            <Projects
              data={data}
              search={search}
              refresh={async () => {}}
              notify={() => {}}
            />
          )}
        </main>
      </div>
    </div>
  );
}
const root = createRoot(document.getElementById("projects-test-root")!);
(window as any).drawProjects = (search = "") =>
  root.render(
    <MemoryRouter>
      <Fixture search={search} />
    </MemoryRouter>,
  );
(window as any).drawProjects();
