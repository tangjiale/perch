import { expect, it } from "vitest";
import {
  buildBugProjectIndex,
  bugProjectKey,
  projectBugKey,
} from "./bug-projects";
import type { Project, ZentaoBug } from "./types";
const project: Project = {
  id: "local-uuid",
  name: "项目",
  source: "zentao",
  connectionId: "a",
  remoteId: "12",
  description: "",
  status: "todo",
  owner: "",
};
const bug = (overrides: Partial<ZentaoBug> = {}): ZentaoBug => ({
  id: "bug",
  title: "BUG",
  connectionId: "a",
  remoteId: "1",
  projectId: "12",
  projectName: "旧名称",
  steps: "",
  assignedTo: "me",
  status: "active",
  severity: 2,
  priority: 2,
  ...overrides,
});
it("按连接和禅道项目编号匹配，不混用本地 UUID、同名或其他连接", () => {
  const groups = buildBugProjectIndex(
    [project],
    [
      bug(),
      bug({ status: "resolved" }),
      bug({ status: "closed" }),
      bug({ connectionId: "b" }),
      bug({ projectId: "local-uuid" }),
    ],
  );
  expect(groups.get(projectBugKey(project)!)).toMatchObject({
    total: 3,
    active: 1,
    resolved: 1,
    closed: 1,
    label: "项目 #12",
  });
  expect(groups.get(bugProjectKey("b", "12"))?.total).toBe(1);
  expect(projectBugKey({ ...project, source: "local" })).toBeUndefined();
});
it("提供零 BUG 项目、缓存外项目和未关联项目，旧快照兼容", () => {
  const groups = buildBugProjectIndex(
    [project],
    [
      bug({ projectId: "99" }),
      bug({ projectId: undefined }),
      bug({ projectId: "0" }),
    ],
  );
  expect(groups.get(projectBugKey(project)!)?.total).toBe(0);
  expect(groups.get(bugProjectKey("a", "99"))?.total).toBe(1);
  expect(groups.get("unassigned")?.total).toBe(2);
  expect(buildBugProjectIndex([project]).size).toBe(1);
  expect(bugProjectKey("a", "0012")).toBe(projectBugKey(project));
});
