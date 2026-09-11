import { command } from "./api";

export interface DingCalendarConfig {
  revision: number;
  serverUrl: string;
  username: string;
  enabled: boolean;
  syncIntervalMinutes: number;
  calendarUrl: string;
  calendarName: string;
  todoCalendarUrl?: string;
  todoCalendarName?: string;
  hasCredential: boolean;
}

export interface DingCalendarEvent {
  id: string;
  title: string;
  description: string;
  location: string;
  start: string;
  end?: string;
  allDay: boolean;
}

export interface DingCalendarTodo {
  id: string;
  title: string;
  description: string;
  location: string;
  start?: string;
  end?: string;
  allDay: boolean;
  status: "todo" | "doing" | "done" | "closed";
}

export interface DingCalendarState {
  config: DingCalendarConfig;
  calendars: { url: string; name: string; components?: string[] }[];
  events: DingCalendarEvent[];
  todos?: DingCalendarTodo[];
  lastSync?: string;
  lastError?: string;
  rangeStart?: string;
  rangeEnd?: string;
}

export const dingCalendarApi = {
  state: () => command<DingCalendarState>("dingtalk_calendar_state"),
  save: (config: DingCalendarConfig, password?: string) =>
    command<DingCalendarState>("dingtalk_calendar_save", {
      config,
      ...(password ? { password } : {}),
    }),
  sync: () => command<DingCalendarState>("dingtalk_calendar_sync"),
};

export function normalizeDingCalendarUrl(input: string): string {
  const value = input.trim();
  if (!value) throw new Error("请填写钉钉提供的 CalDAV 服务器地址");
  let url: URL;
  try {
    url = new URL(value.includes("://") ? value : `https://${value}`);
  } catch {
    throw new Error("服务器地址格式不正确，请核对钉钉提供的地址");
  }
  if (url.protocol !== "https:" || !url.hostname || url.username || url.password || url.hash || url.search) {
    throw new Error("请使用 HTTPS 服务器地址，地址中不能包含用户名、密码或锚点");
  }
  return url.toString();
}

/** 只报告接口实际读取的条数，不把连接成功描述为内置待办已接入。 */
export function dingCalendarSyncSummary(state: DingCalendarState): string {
  if (state.lastError) return "本次同步未完成，已有日程保留";
  return state.config.calendarUrl
    ? `本次读取：日程 ${state.events.length} 条`
    : "已获取日历，请选择“我的日历”后保存并同步";
}
