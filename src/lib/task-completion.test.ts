import { DateTime } from "luxon";
import { describe, expect, it } from "vitest";
import { buildTaskCompletionReview } from "./task-completion";
import type { Task } from "./types";

const date = (iso: string, zone = "Asia/Shanghai") =>
  DateTime.fromISO(iso, { zone });
const task = (id: string, completedAt?: number | null, status: Task["status"] = "done"): Task => ({
  id, title: id, notes: "", source: "local", status,
  priority: "normal", sortOrder: 0, completedAt,
});
const ids = (tasks: Task[]) => tasks.map(({ id }) => id);

describe("任务完成周回顾", () => {
  it("按周一划分本周与上周，排除更早及未来时间", () => {
    const now = date("2026-10-18T23:59:59");
    const result = buildTaskCompletionReview([
      task("last-start", date("2026-10-05T00:00:00").toMillis()),
      task("last-end", date("2026-10-11T23:59:59.999").toMillis()),
      task("this-start", date("2026-10-12T00:00:00").toMillis()),
      task("old", date("2026-10-04T23:59:59.999").toMillis()),
      task("future", now.plus({ milliseconds: 1 }).toMillis()),
      task("at-now", now.toMillis()),
    ], now);
    expect(ids(result.thisWeek.tasks)).toEqual(["at-now", "this-start"]);
    expect(ids(result.lastWeek.tasks)).toEqual(["last-end", "last-start"]);
    expect(result.thisWeek.end.toISODate()).toBe("2026-10-19");
    expect(result.unknown).toEqual([]);
  });

  it("周一零点本周为空，跨年仍使用完整自然周", () => {
    const result = buildTaskCompletionReview([
      task("sunday", date("2026-01-04T23:59:59.999").toMillis()),
      task("december", date("2025-12-29T00:00:00").toMillis()),
    ], date("2026-01-05T00:00:00"));
    expect(result.thisWeek.tasks).toEqual([]);
    expect(ids(result.lastWeek.tasks)).toEqual(["sunday", "december"]);
    expect(result.lastWeek.start.toISODate()).toBe("2025-12-29");
    expect(result.lastWeek.end.toISODate()).toBe("2026-01-05");
  });

  it("以 now 时区分组，同一瞬间不受时间戳原始偏移影响", () => {
    const instant = DateTime.fromISO("2026-10-11T17:00:00Z").toMillis();
    const shanghai = buildTaskCompletionReview([task("one", instant)], date("2026-10-14T12:00:00"));
    const utc = buildTaskCompletionReview([task("one", instant)], date("2026-10-14T12:00:00", "UTC"));
    expect(ids(shanghai.thisWeek.tasks)).toEqual(["one"]);
    expect(ids(utc.lastWeek.tasks)).toEqual(["one"]);
  });

  it.each([
    ["2026-03-09T12:00:00", 167],
    ["2026-11-02T12:00:00", 169],
  ])("夏令时自然周 %s 不固定为 168 小时", (iso, hours) => {
    const result = buildTaskCompletionReview([], date(iso, "America/New_York"));
    expect(result.lastWeek.end.diff(result.lastWeek.start, "hours").hours).toBe(hours);
    expect(result.lastWeek.start.weekday).toBe(1);
    expect(result.lastWeek.end.weekday).toBe(1);
  });

  it("缺失和无效完成时间单列，重开的任务不计入且不推断时间", () => {
    const now = date("2026-10-16T12:00:00");
    const missing = { ...task("missing"), updatedAt: now.minus({ days: 1 }).toMillis() };
    const result = buildTaskCompletionReview([
      missing, task("null", null), task("nan", NaN), task("infinity", Infinity),
      task("invalid", 9e15),
      task("reopened", now.minus({ days: 1 }).toMillis(), "doing"),
    ], now);
    expect(ids(result.unknown)).toEqual(["infinity", "invalid", "missing", "nan", "null"]);
    expect(result.thisWeek.tasks).toEqual([]);
    expect(result.lastWeek.tasks).toEqual([]);
  });

  it("禅道仅按可信来源归周，旧版同步时间及非法来源单列", () => {
    const now = date("2026-10-16T12:00:00");
    const completedAt = now.minus({ days: 1 }).toMillis();
    const result = buildTaskCompletionReview([
      { ...task("legacy", completedAt), source: "zentao" },
      { ...task("invalid-source", completedAt), source: "zentao", completedAtSource: "sync" } as unknown as Task,
      { ...task("local-close", completedAt), source: "zentao", completedAtSource: "local" },
      { ...task("remote-close", completedAt), source: "zentao", completedAtSource: "remote" },
      task("personal", completedAt),
    ], now);
    expect(ids(result.unknown)).toEqual(["invalid-source", "legacy"]);
    expect(ids(result.thisWeek.tasks)).toEqual(["local-close", "personal", "remote-close"]);
  });

  it("完成时间倒序，同一时间按 ID 升序，保持输入数组及对象不变", () => {
    const now = date("2026-10-16T12:00:00");
    const earlier = now.minus({ days: 2 }).toMillis();
    const later = now.minus({ days: 1 }).toMillis();
    const input = [task("z", earlier), task("b", later), task("a", later)];
    input.forEach(Object.freeze);
    Object.freeze(input);
    const result = buildTaskCompletionReview(input, now);
    expect(ids(result.thisWeek.tasks)).toEqual(["a", "b", "z"]);
    expect(ids(input)).toEqual(["z", "b", "a"]);
    expect(result.thisWeek.tasks[0]).toBe(input[2]);
  });

  it("拒绝无效当前时间", () => {
    expect(() => buildTaskCompletionReview([], DateTime.invalid("test"))).toThrow("有效的当前时间");
  });
});
