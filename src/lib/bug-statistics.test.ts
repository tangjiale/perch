import { describe, expect, it } from "vitest";
import { buildBugStatistics } from "./bug-statistics";
import type { ZentaoBug } from "./types";

const bug = (value: Partial<ZentaoBug> = {}): ZentaoBug => ({
  id: "bug-1",
  revision: 1,
  createdAt: Date.parse("2026-09-08T00:00:00Z"),
  updatedAt: Date.parse("2026-09-08T00:00:00Z"),
  connectionId: "connection",
  remoteId: "1",
  title: "测试",
  steps: "",
  status: "active",
  assignedTo: "me",
  severity: 2,
  priority: 1,
  ...value,
});
const now = Date.parse("2026-09-08T09:00:00Z");
describe("个人 BUG 统计", () => {
  it("旧快照和空缓存不产生虚构活动", () => {
    const stats = buildBugStatistics(undefined, 7, now, "Asia/Shanghai");
    expect(stats.total).toBe(0);
    expect(stats.trend).toHaveLength(7);
    expect(
      stats.trend.every((row) => row.opened === 0 && row.resolved === 0),
    ).toBe(true);
  });
  it("严重待解决不包含已解决或已关闭，产品按 ID 分组", () => {
    const stats = buildBugStatistics(
      [
        bug({ productId: "1", productName: "同名" }),
        bug({
          id: "2",
          status: "resolved",
          productId: "2",
          productName: "同名",
        }),
        bug({
          id: "3",
          status: "closed",
          severity: 1,
          productId: "1",
          productName: "同名",
        }),
      ],
      7,
      now,
      "Asia/Shanghai",
    );
    expect([
      stats.total,
      stats.active,
      stats.resolved,
      stats.closed,
      stats.urgent,
    ]).toEqual([3, 1, 1, 1, 1]);
    expect(stats.products.map((p) => p.value)).toEqual([2, 1]);
    expect(new Set(stats.products.map((p) => p.name)).size).toBe(2);
    expect(stats.trend.every((row) => row.opened === 0)).toBe(true);
  });
  it("采用远端日期和指定时区，排除无效及范围外时间", () => {
    const stats = buildBugStatistics(
      [
        bug({
          openedDate: "2026-09-07T20:00:00Z",
          resolvedDate: "2026-09-08 12:00:00",
        }),
        bug({ openedDate: "0000-00-00 00:00:00", resolvedDate: "invalid" }),
        bug({ openedDate: "2026-08-01", resolvedDate: "2026-09-09" }),
      ],
      7,
      now,
      "Asia/Shanghai",
    );
    expect(stats.trend.at(-1)).toEqual({
      date: "2026-09-08",
      opened: 1,
      resolved: 1,
    });
    expect(
      stats.trend.reduce((sum, row) => sum + row.opened + row.resolved, 0),
    ).toBe(2);
  });
});
