import { expect, it } from "vitest";
import { calendarAlmanac, hasHolidaySchedule } from "./calendar-almanac";

it("按年度安排区分中秋、国庆放假与调休上班", () => {
  expect(calendarAlmanac(2026, 9, 20).kind).toBe("work");
  for (const day of [25, 26, 27]) expect(calendarAlmanac(2026, 9, day).kind).toBe("rest");
  expect(calendarAlmanac(2026, 9, 25).label).toBe("中秋节");
  for (const day of [1, 2, 3, 4, 5, 6, 7]) expect(calendarAlmanac(2026, 10, day).kind).toBe("rest");
  expect(calendarAlmanac(2026, 10, 10).kind).toBe("work");
  expect(calendarAlmanac(2026, 10, 10).title).toContain("调休上班");
  expect(calendarAlmanac(2026, 10, 1).title).toContain("国庆节放假");
});

it("普通周六、周日标记休息，普通工作日不标记", () => {
  for (const [month, day] of [[9, 12], [10, 11], [10, 17], [10, 18], [10, 24], [10, 25]]) {
    const almanac = calendarAlmanac(2026, month, day);
    expect(almanac.kind).toBe("rest");
    expect(almanac.title).toContain("周末休息");
    expect(almanac.title).not.toMatch(/国庆|放假/);
  }
  for (const day of [12, 13, 14, 15, 16]) {
    expect(calendarAlmanac(2026, 10, day).kind).toBeUndefined();
    expect(calendarAlmanac(2026, 10, day).title).not.toContain("周末休息");
  }
});

it("显示农历节气", () => {
  expect(calendarAlmanac(2026, 9, 10).label).toBe("廿九");
  expect(calendarAlmanac(2026, 9, 23).label).toBe("秋分");
});

it("未收录年份仍显示周末休息，但不推算官方放假或调休", () => {
  expect(hasHolidaySchedule(2026)).toBe(true);
  expect(hasHolidaySchedule(2001)).toBe(false);
  expect(hasHolidaySchedule(2099)).toBe(false);
  expect(calendarAlmanac(2099, 10, 1).kind).toBeUndefined();
  expect(calendarAlmanac(2001, 10, 1).kind).toBeUndefined();
  for (const [year, day] of [[2001, 6], [2001, 7], [2099, 3], [2099, 4]]) {
    const almanac = calendarAlmanac(year, 10, day);
    expect(almanac.kind).toBe("rest");
    expect(almanac.title).toContain("周末休息");
    expect(almanac.title).not.toMatch(/放假|调休上班/);
  }
});
