import { HolidayUtil, Solar } from "lunar-typescript";

export function calendarAlmanac(year: number, month: number, day: number) {
  const solar = Solar.fromYmd(year, month, day);
  const lunar = solar.getLunar();
  const holiday = year >= 2002 ? HolidayUtil.getHoliday(year, month, day) : null;
  const lunarDate = `${lunar.getMonthInChinese()}月${lunar.getDayInChinese()}`;
  const label = lunar.getFestivals()[0] || lunar.getJieQi()
    || (lunar.getDay() === 1 ? `${lunar.getMonthInChinese()}月` : lunar.getDayInChinese());
  const weekend = solar.getWeek() === 0 || solar.getWeek() === 6;
  // 年度调休安排优先，未另行安排的周末按休息日展示。
  const kind = holiday ? (holiday.isWork() ? "work" : "rest") : weekend ? "rest" : undefined;
  const restLabel = holiday
    ? `${holiday.getName()}${holiday.isWork() ? "调休上班" : "放假"}`
    : weekend ? "周末休息" : "";
  return {
    label,
    kind,
    title: `农历${lunarDate}${restLabel ? ` · ${restLabel}` : ""}`,
  };
}

export function hasHolidaySchedule(year: number) {
  return year >= 2002 && HolidayUtil.getHolidays(year).length > 0;
}
