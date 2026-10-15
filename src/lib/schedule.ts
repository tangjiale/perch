import { DateTime } from "luxon";
import type { Task } from "./types";

export function createSchedule(
  start: string,
  end: string,
  timezone: string,
  allDay: boolean,
): Task["schedule"] {
  if (end && !start) throw Error("请先填写开始日期时间");
  if (!start) return undefined;
  const a = DateTime.fromISO(start, { zone: timezone });
  const b = end ? DateTime.fromISO(end, { zone: timezone }) : undefined;
  if (!a.isValid || (b && !b.isValid)) throw Error("日期或时区无效");
  if (
    !allDay &&
    (a.toFormat("yyyy-MM-dd'T'HH:mm") !== start ||
      (b && b.toFormat("yyyy-MM-dd'T'HH:mm") !== end))
  )
    throw Error("该时间因夏令时切换不存在，请选择其他时间");
  if (
    !allDay &&
    (a.getPossibleOffsets().length > 1 ||
      (b && b.getPossibleOffsets().length > 1))
  )
    throw Error("该时间处于夏令时重复时段，请选择明确的其他时间");
  if (b && (allDay ? b < a : b <= a)) throw Error("结束时间必须晚于开始时间");
  return allDay
    ? {
        kind: "all_day",
        timezone,
        start: a.toISODate()!,
        end: b?.plus({ days: 1 }).toISODate() || undefined,
      }
    : {
        kind: "timed",
        timezone,
        start: a.toISO()!,
        end: b?.toISO() || undefined,
      };
}

/** 全天排期的 end 为次日排他边界，展示时还原用户填写的结束日。 */
export function taskScheduleDates(schedule: Task["schedule"]) {
  const format = (raw?: string, end = false) => {
    if (!raw || !schedule) return "未设置";
    let date = DateTime.fromISO(raw, { zone: schedule.timezone });
    if (!date.isValid) return "未设置";
    if (end && schedule.kind === "all_day") date = date.minus({ days: 1 });
    return date.toFormat(
      schedule.kind === "all_day" ? "yyyy-MM-dd" : "yyyy-MM-dd HH:mm",
    );
  };
  return { start: format(schedule?.start), end: format(schedule?.end, true) };
}

/** 列表紧凑展示完整排期，全天结束日还原为用户选择的包含日期。 */
export function taskScheduleLabel(schedule: Task["schedule"]): string {
  if (!schedule) return "未排期";
  const start = DateTime.fromISO(schedule.start, { zone: schedule.timezone });
  if (!start.isValid) return "未排期";
  let end = schedule.end
    ? DateTime.fromISO(schedule.end, { zone: schedule.timezone })
    : undefined;
  if (end && schedule.kind === "all_day") end = end.minus({ days: 1 });
  if (end && (!end.isValid || end < start)) end = undefined;
  const dateFormat = end && end.year !== start.year ? "yyyy年M月d日" : "M月d日";
  if (schedule.kind === "all_day") {
    return (
      start.toFormat(dateFormat) +
      (end && !end.hasSame(start, "day") ? ` – ${end.toFormat(dateFormat)}` : "") +
      " 全天"
    );
  }
  return (
    start.toFormat(`${dateFormat} HH:mm`) +
    (end
      ? ` – ${end.toFormat(end.hasSame(start, "day") ? "HH:mm" : `${dateFormat} HH:mm`)}`
      : "")
  );
}
