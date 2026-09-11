import { expect, it } from "vitest";
import { sortBugs } from "./bug-sort";
import type { ZentaoBug } from "./types";

it("按远端日期排序，缺失或无效日期始终置后且不修改原数组", () => {
  const bugs = [
    { id: "old", openedDate: "2026-08-10 13:19:00", createdAt: 9999999999999 },
    { id: "missing" },
    { id: "new", openedDate: "2026-09-10 10:00:00" },
    { id: "invalid", openedDate: "0000-00-00" },
  ] as ZentaoBug[];
  expect(sortBugs(bugs, "openedDate", "desc").map((b) => b.id)).toEqual(["new", "old", "invalid", "missing"]);
  expect(sortBugs(bugs, "openedDate", "asc").slice(0, 2).map((b) => b.id)).toEqual(["old", "new"]);
  expect(bugs[0].id).toBe("old");
});

it("状态和数值按业务顺序切换，截止日期空值始终置后", () => {
  const bugs = [
    { id: "a", status: "closed", priority: 3, severity: 2, deadline: "2026-09-15" },
    { id: "b", status: "active", priority: 1, severity: 3 },
    { id: "c", status: "resolved", priority: 2, severity: 1, deadline: "2026-09-12" },
  ] as ZentaoBug[];
  expect(sortBugs(bugs, "status", "asc").map((b) => b.id)).toEqual(["b", "c", "a"]);
  expect(sortBugs(bugs, "priority", "desc").map((b) => b.id)).toEqual(["a", "c", "b"]);
  expect(sortBugs(bugs, "severity", "asc").map((b) => b.id)).toEqual(["c", "a", "b"]);
  expect(sortBugs(bugs, "deadline", "asc").map((b) => b.id)).toEqual(["c", "a", "b"]);
  expect(sortBugs(bugs, "deadline", "desc").map((b) => b.id)).toEqual(["a", "c", "b"]);
});
