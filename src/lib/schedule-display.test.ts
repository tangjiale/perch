import { expect, it } from "vitest";
import { taskScheduleDates } from "./schedule";
it("全天结束边界还原日期，支持同一天和跨年", () => {
  expect(
    taskScheduleDates({
      kind: "all_day",
      timezone: "Asia/Shanghai",
      start: "2026-09-23",
      end: "2026-09-24",
    }),
  ).toEqual({ start: "2026-09-23", end: "2026-09-23" });
  expect(
    taskScheduleDates({
      kind: "all_day",
      timezone: "Asia/Shanghai",
      start: "2026-12-31",
      end: "2027-01-03",
    }).end,
  ).toBe("2027-01-02");
});
it("定时任务按排期时区展示，缺失或无效日期不补造", () => {
  expect(
    taskScheduleDates({
      kind: "timed",
      timezone: "Asia/Shanghai",
      start: "2026-09-09T01:00:00Z",
      end: "2026-09-09T02:00:00Z",
    }),
  ).toEqual({ start: "2026-09-09 09:00", end: "2026-09-09 10:00" });
  expect(taskScheduleDates(undefined)).toEqual({
    start: "未设置",
    end: "未设置",
  });
  expect(
    taskScheduleDates({
      kind: "all_day",
      timezone: "Asia/Shanghai",
      start: "2026-09-09",
      end: "invalid",
    }).end,
  ).toBe("未设置");
});
