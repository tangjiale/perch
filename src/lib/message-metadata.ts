import type { Message } from "./types";
const count = (value?: number) =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0
    ? value
    : undefined;
export function messageMetadata(message: Message) {
  const input = count(message.usage?.inputTokens);
  const output = count(message.usage?.outputTokens);
  const total =
    count(message.usage?.totalTokens) ??
    (input !== undefined && output !== undefined
      ? count(input + output)
      : undefined);
  const running =
    message.status === "streaming" || message.status === "pending";
  const duration = message.durationMs;
  const seconds =
    duration !== undefined && Number.isFinite(duration) && duration >= 0
      ? Math.round(duration / 100) / 10
      : undefined;
  const timestamp = message.finishedAt ?? message.createdAt;
  const date =
    timestamp !== undefined && Number.isFinite(timestamp) && timestamp > 0
      ? new Date(timestamp)
      : undefined;
  const validDate = date && Number.isFinite(date.getTime()) ? date : undefined;
  const number = (value: number) => value.toLocaleString("zh-CN");
  return {
    usage:
      total !== undefined
        ? `用量 ${total >= 1000 ? `${(total / 1000).toFixed(2)}K` : number(total)} tok`
        : running
          ? "用量统计中"
          : "用量未提供",
    usageDetail: `输入 ${input === undefined ? "未提供" : number(input)} · 输出 ${output === undefined ? "未提供" : number(output)}`,
    duration:
      seconds !== undefined
        ? `用时 ${seconds >= 60 ? `${Math.floor(seconds / 60)} 分 ${Math.round((seconds % 60) * 10) / 10} 秒` : `${seconds} 秒`}`
        : running
          ? "生成中"
          : "耗时未记录",
    time: validDate?.toLocaleTimeString("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    }),
    isoTime: validDate?.toISOString(),
    fullTime: validDate?.toLocaleString("zh-CN", { hour12: false }),
  };
}
