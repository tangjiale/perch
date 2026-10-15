import { describe, expect, it } from "vitest";
import { taskScheduleLabel } from "./schedule";
import type { Schedule } from "./types";
const allDay = (start: string, end?: string): Schedule => ({ kind: "all_day", timezone: "Asia/Shanghai", start, end });
const timed = (start: string, end?: string): Schedule => ({ kind: "timed", timezone: "Asia/Shanghai", start, end });
describe("个人安排起止日期展示", () => {
  it("跨日全天结束采用排他日期的前一天", () => expect(taskScheduleLabel(allDay("2026-10-12", "2026-10-16"))).toBe("10月12日 – 10月15日 全天"));
  it("单日全天日期不重复", () => expect(taskScheduleLabel(allDay("2026-10-12", "2026-10-13"))).toBe("10月12日 全天"));
  it("跨年明确显示两侧年份", () => expect(taskScheduleLabel(allDay("2026-12-31", "2027-01-03"))).toBe("2026年12月31日 – 2027年1月2日 全天"));
  it("跨月包含两侧月份", () => expect(taskScheduleLabel(allDay("2026-10-31", "2026-11-03"))).toBe("10月31日 – 11月2日 全天"));
  it("闰年排他结束边界正确", () => expect(taskScheduleLabel(allDay("2028-02-28", "2028-03-01"))).toBe("2月28日 – 2月29日 全天"));
  for (const end of [undefined, "bad-date", "2026-10-12", "2026-10-11"]) it(`全天结束缺失或无效不捏造范围 ${end}`, () => expect(taskScheduleLabel(allDay("2026-10-12", end))).toBe("10月12日 全天"));
  it("未排期", () => expect(taskScheduleLabel(undefined)).toBe("未排期"));
  it("timed保留指定时区并同日仅显示一次日期", () => expect(taskScheduleLabel(timed("2026-10-12T01:00:00Z", "2026-10-12T02:00:00Z"))).toBe("10月12日 09:00 – 10:00"));
  it("timed跨日保留结束日期", () => expect(taskScheduleLabel(timed("2026-10-12T15:00:00Z", "2026-10-12T17:00:00Z"))).toBe("10月12日 23:00 – 10月13日 01:00"));
  for (const end of [undefined, "bad-date"]) it(`timed缺失或无效结束只显示开始 ${end}`, () => expect(taskScheduleLabel(timed("2026-10-12T01:00:00Z", end))).toBe("10月12日 09:00"));
});
