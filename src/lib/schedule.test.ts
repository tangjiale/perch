import { describe, expect, it } from "vitest";
import { createSchedule } from "./schedule";

describe("任务排期", () => {
  it("未排期与缺失开始时间", () => {
    expect(createSchedule("", "", "Asia/Shanghai", false)).toBeUndefined();
    expect(() =>
      createSchedule("", "2026-09-07T12:00", "Asia/Shanghai", false),
    ).toThrow("开始");
  });
  it("全天结束日期使用排他边界", () => {
    expect(
      createSchedule("2026-09-07", "2026-09-09", "Asia/Shanghai", true),
    ).toMatchObject({ start: "2026-09-07", end: "2026-09-10" });
    expect(
      createSchedule("2026-09-07", "", "Asia/Shanghai", true)?.end,
    ).toBeUndefined();
  });
  it("跨日定时任务保留偏移和结束日期", () => {
    const result = createSchedule(
      "2026-09-07T23:00",
      "2026-09-08T02:00",
      "Asia/Shanghai",
      false,
    );
    expect(result?.start).toBe("2026-09-07T23:00:00.000+08:00");
    expect(result?.end).toBe("2026-09-08T02:00:00.000+08:00");
  });
  it("拒绝倒置、相等时间与无效时区", () => {
    expect(() =>
      createSchedule(
        "2026-09-07T12:00",
        "2026-09-07T12:00",
        "Asia/Shanghai",
        false,
      ),
    ).toThrow("结束");
    expect(() =>
      createSchedule("2026-09-08", "2026-09-07", "Asia/Shanghai", true),
    ).toThrow("结束");
    expect(() =>
      createSchedule("2026-09-07", "", "invalid/zone", true),
    ).toThrow("无效");
  });
  it("拒绝夏令时跳过和重复的本地时刻", () => {
    expect(() =>
      createSchedule("2026-03-08T02:30", "", "America/New_York", false),
    ).toThrow("不存在");
    expect(() =>
      createSchedule("2026-11-01T01:30", "", "America/New_York", false),
    ).toThrow("重复");
  });
});
