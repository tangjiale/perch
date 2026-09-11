import { describe, it, expect } from "vitest";
import { DateTime } from "luxon";
import { isTaskExpired } from "./task-expiry";
import type { Task } from "./types";

const now = DateTime.fromISO("2026-09-09T00:00:00+08:00", { setZone: true });
const base: Task = {
  id: "task",
  title: "开发",
  notes: "",
  source: "zentao",
  status: "todo",
  priority: "normal",
  sortOrder: 0,
};
describe("任务过期标签", () => {
  it("已关闭不标记过期，其他状态仍按结束时间判断", () => {
    for (const status of ["todo", "doing", "done", "closed"] as const)
      expect(
        isTaskExpired(
          {
            ...base,
            status,
            schedule: {
              kind: "all_day",
              timezone: "Asia/Shanghai",
              start: "2026-09-08",
              end: "2026-09-09",
            },
          },
          now,
        ),
      ).toBe(status !== "closed");
  });
  it("截止当天未过期，跨日全天范围在结束后过期", () => {
    expect(
      isTaskExpired(
        {
          ...base,
          schedule: {
            kind: "all_day",
            timezone: "Asia/Shanghai",
            start: "2026-09-09",
            end: "2026-09-10",
          },
        },
        now,
      ),
    ).toBe(false);
    const schedule = {
      kind: "all_day" as const,
      timezone: "Asia/Shanghai",
      start: "2026-09-07",
      end: "2026-09-09",
    };
    expect(isTaskExpired({ ...base, schedule }, now)).toBe(true);
    expect(
      isTaskExpired({ ...base, schedule }, now.minus({ minutes: 1 })),
    ).toBe(false);
  });
  it("定时任务按结束时刻，没有结束时间不判断过期", () => {
    const schedule = {
      kind: "timed" as const,
      timezone: "Asia/Shanghai",
      start: "2026-09-08T08:00:00+08:00",
      end: "2026-09-08T18:00:00+08:00",
    };
    expect(isTaskExpired({ ...base, schedule }, now)).toBe(true);
    expect(
      isTaskExpired(
        { ...base, schedule: { ...schedule, end: undefined } },
        now,
      ),
    ).toBe(false);
    expect(isTaskExpired(base, now)).toBe(false);
    expect(
      isTaskExpired(
        {
          ...base,
          schedule: {
            kind: "all_day",
            timezone: "Asia/Shanghai",
            start: "2020-01-01",
          },
        },
        now,
      ),
    ).toBe(false);
  });
});
