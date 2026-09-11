import { DateTime } from "luxon";
import type { ZentaoBug } from "./types";

export type BugSortField = "status" | "priority" | "severity" | "openedDate" | "deadline";
export type BugSortDirection = "asc" | "desc";

function valueOf(bug: ZentaoBug, field: BugSortField): number | undefined {
  if (field === "status") return { active: 0, resolved: 1, closed: 2 }[bug.status];
  if (field === "priority" || field === "severity") {
    return Number.isFinite(bug[field]) ? bug[field] : undefined;
  }
  const value = bug[field];
  if (!value) return undefined;
  const date = DateTime.fromISO(value.replace(" ", "T"));
  return date.isValid ? date.toMillis() : undefined;
}

export function sortBugs(bugs: ZentaoBug[], field: BugSortField, direction: BugSortDirection) {
  // 使用禅道业务创建时间，不能用本地导入时间代替；空日期始终排末尾。
  const compare = (a: number | undefined, b: number | undefined, order: BugSortDirection) => {
    if (a === undefined) return b === undefined ? 0 : 1;
    if (b === undefined) return -1;
    return order === "asc" ? a - b : b - a;
  };
  return bugs.map((bug) => ({ bug, value: valueOf(bug, field), created: valueOf(bug, "openedDate") }))
    .sort((a, b) => compare(a.value, b.value, direction)
      || compare(a.created, b.created, "desc")
      || a.bug.id.localeCompare(b.bug.id, "en", { numeric: true }))
    .map(({ bug }) => bug);
}
