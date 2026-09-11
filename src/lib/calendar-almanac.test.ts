import { expect, it } from "vitest";
import { calendarAlmanac, hasHolidaySchedule } from "./calendar-almanac";

it("按年度安排区分中秋、国庆放假与调休上班，不把普通周末当作假期", () => {
  expect(calendarAlmanac(2026, 9, 20).kind).toBe("work");
  for (const day of [25, 26, 27]) expect(calendarAlmanac(2026, 9, day).kind).toBe("rest");
  expect(calendarAlmanac(2026, 9, 25).label).toBe("中秋节");
  for (const day of [1, 2, 3, 4, 5, 6, 7]) expect(calendarAlmanac(2026, 10, day).kind).toBe("rest");
  expect(calendarAlmanac(2026, 10, 10).kind).toBe("work");
  expect(calendarAlmanac(2026, 9, 12).kind).toBeUndefined();
});

it("显示农历节气，不对未收录年份推算放假或调休", () => {
  expect(calendarAlmanac(2026, 9, 10).label).toBe("廿九");
  expect(calendarAlmanac(2026, 9, 23).label).toBe("秋分");
  expect(hasHolidaySchedule(2026)).toBe(true);
  expect(hasHolidaySchedule(2001)).toBe(false);
  expect(hasHolidaySchedule(2099)).toBe(false);
  expect(calendarAlmanac(2099, 10, 1).kind).toBeUndefined();
});
