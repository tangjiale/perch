import { DateTime } from "luxon";
import type { Task } from "./types";

export function isTaskExpired(
  task: Task,
  now: DateTime = DateTime.now(),
): boolean {
  if (task.status === "closed") return false;
  const zone = task.schedule?.timezone || now.zoneName || "Asia/Shanghai";
  const current = now.setZone(zone);
  if (!current.isValid) return false;
  const schedule = task.schedule;
  if (schedule) {
    const end = schedule.end ? DateTime.fromISO(schedule.end, { zone }) : null;
    return !!end?.isValid && current >= end;
  }
  return false;
}
