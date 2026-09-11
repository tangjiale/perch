import type { Project, ZentaoBug } from "./types";

export function bugProjectKey(
  connectionId?: string,
  remoteId?: string,
): string {
  const id = remoteId?.replace(/^0+/, "");
  return connectionId && id && /^\d+$/.test(id)
    ? JSON.stringify([connectionId, id])
    : "unassigned";
}

export function projectBugKey(project: Project): string | undefined {
  if (project.source !== "zentao") return undefined;
  const key = bugProjectKey(project.connectionId, project.remoteId);
  return key === "unassigned" ? undefined : key;
}

export function buildBugProjectIndex(
  projects: Project[],
  bugs: ZentaoBug[] = [],
) {
  const groups = new Map<
    string,
    {
      label: string;
      total: number;
      active: number;
      resolved: number;
      closed: number;
    }
  >();
  const add = (key: string, label: string) => {
    if (!groups.has(key))
      groups.set(key, { label, total: 0, active: 0, resolved: 0, closed: 0 });
    return groups.get(key)!;
  };
  for (const project of projects) {
    const key = projectBugKey(project);
    if (key) add(key, `${project.name} #${project.remoteId}`);
  }
  for (const bug of bugs) {
    const key = bugProjectKey(bug.connectionId, bug.projectId);
    const group = add(
      key,
      key === "unassigned"
        ? "未关联项目"
        : `${bug.projectName || "禅道项目"} #${bug.projectId}`,
    );
    group.total++;
    group[bug.status]++;
  }
  // 同名同编号但来自不同连接时仍需能区分，不按名称合并关联。
  const labels = new Map<string, number>();
  for (const group of groups.values())
    labels.set(group.label, (labels.get(group.label) || 0) + 1);
  for (const [key, group] of groups) {
    if ((labels.get(group.label) || 0) > 1 && key !== "unassigned")
      group.label += ` · ${JSON.parse(key)[0]}`;
  }
  return groups;
}
