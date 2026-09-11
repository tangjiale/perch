import { expect, it } from "vitest";
import { messageMetadata } from "./message-metadata";
import type { Message } from "./types";
const message: Message = {
  id: "m",
  conversationId: "c",
  role: "assistant",
  content: "你好",
  status: "completed",
};
it("展示实际输入输出总量、耗时和完成时间", () => {
  const result = messageMetadata({
    ...message,
    usage: { inputTokens: 12000, outputTokens: 700 },
    durationMs: 2100,
    finishedAt: 1788919080000,
  });
  expect(result.usage).toBe("用量 12.70K tok");
  expect(result.duration).toBe("用时 2.1 秒");
  expect(result.isoTime).toBe(new Date(1788919080000).toISOString());
  expect(result.usageDetail).toContain("12,000");
  for (const [totalTokens, display] of [
    [999, "999"],
    [1000, "1.00K"],
    [7460, "7.46K"],
    [7466, "7.47K"],
  ] as const) {
    expect(messageMetadata({ ...message, usage: { totalTokens } }).usage)
      .toBe(`用量 ${display} tok`);
  }
});
it("旧消息或不完整用量不补造数据，零值仍有效", () => {
  expect(messageMetadata(message)).toMatchObject({
    usage: "用量未提供",
    duration: "耗时未记录",
  });
  expect(
    messageMetadata({ ...message, usage: { inputTokens: 12 } }).usage,
  ).toBe("用量未提供");
  expect(
    messageMetadata({ ...message, usage: { totalTokens: 0 }, durationMs: 0 }),
  ).toMatchObject({ usage: "用量 0 tok", duration: "用时 0 秒" });
  expect(
    messageMetadata({
      ...message,
      usage: { totalTokens: -1 },
      durationMs: NaN,
      finishedAt: Infinity,
    }),
  ).toMatchObject({
    usage: "用量未提供",
    duration: "耗时未记录",
    time: undefined,
  });
});
