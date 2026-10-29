import { DateTime } from "luxon";
import type { Task } from "./types";

export interface TaskCompletionWeek {
  start: DateTime;
  /** 自然周的排他上界。 */
  end: DateTime;
  tasks: Task[];
}

export function buildTaskCompletionReview(
  tasks: Task[],
  now: DateTime = DateTime.local(),
): {
  thisWeek: TaskCompletionWeek;
  lastWeek: TaskCompletionWeek;
  unknown: Task[];
} {
  if (!now.isValid) throw new Error("完成回顾需要有效的当前时间");
  // 按设备时区的自然周计算，不能用固定毫秒数跨越夏令时。
  const start = now.startOf("week");
  const thisWeek: TaskCompletionWeek = {
    start,
    end: start.plus({ weeks: 1 }),
    tasks: [],
  };
  const lastWeek: TaskCompletionWeek = {
    start: start.minus({ weeks: 1 }),
    end: start,
    tasks: [],
  };
  const unknown: Task[] = [];

  for (const task of tasks) {
    if (task.status !== "done") continue;
    const completedAt = task.completedAt;
    if (
      // 旧版禅道完成时间可能来自同步时刻，须待同步校正来源后再归周。
      (task.source === "zentao" &&
        task.completedAtSource !== "local" &&
        task.completedAtSource !== "remote") ||
      typeof completedAt !== "number" ||
      !Number.isFinite(completedAt) ||
      !DateTime.fromMillis(completedAt, { zone: now.zone }).isValid
    ) {
      unknown.push(task);
      continue;
    }
    if (completedAt > now.toMillis()) continue;
    if (
      completedAt >= start.toMillis() &&
      completedAt < thisWeek.end.toMillis()
    ) {
      thisWeek.tasks.push(task);
    } else if (
      completedAt >= lastWeek.start.toMillis() &&
      completedAt < start.toMillis()
    ) {
      lastWeek.tasks.push(task);
    }
  }
  const byId = (a: Task, b: Task) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
  const byCompletion = (a: Task, b: Task) =>
    (b.completedAt as number) - (a.completedAt as number) || byId(a, b);
  thisWeek.tasks.sort(byCompletion);
  lastWeek.tasks.sort(byCompletion);
  unknown.sort(byId);
  return { thisWeek, lastWeek, unknown };
}
