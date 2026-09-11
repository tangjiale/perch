import { HolidayUtil, Solar } from "lunar-typescript";

export function calendarAlmanac(year: number, month: number, day: number) {
  const lunar = Solar.fromYmd(year, month, day).getLunar();
  const holiday = year >= 2002 ? HolidayUtil.getHoliday(year, month, day) : null;
  const lunarDate = `${lunar.getMonthInChinese()}月${lunar.getDayInChinese()}`;
  const label = lunar.getFestivals()[0] || lunar.getJieQi()
    || (lunar.getDay() === 1 ? `${lunar.getMonthInChinese()}月` : lunar.getDayInChinese());
  const kind = holiday ? (holiday.isWork() ? "work" : "rest") : undefined;
  return {
    label,
    kind,
    title: `农历${lunarDate}${holiday ? ` · ${holiday.getName()}${holiday.isWork() ? "调休上班" : "放假"}` : ""}`,
  };
}

export function hasHolidaySchedule(year: number) {
  return year >= 2002 && HolidayUtil.getHolidays(year).length > 0;
}
