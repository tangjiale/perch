export type AppearanceMode = "light" | "dark" | "system";
export interface AppearancePreferences {
  mode: AppearanceMode;
  chatFontSize: number;
}

export const APPEARANCE_KEY = "perch.appearance.v1";
const defaults: AppearancePreferences = { mode: "system", chatFontSize: 14 };
let current = defaults;
const listeners = new Set<() => void>();

export function normalizeAppearance(value: unknown): AppearancePreferences {
  const record =
    value && typeof value === "object"
      ? (value as Record<string, unknown>)
      : {};
  return {
    mode: ["light", "dark", "system"].includes(record.mode as string)
      ? (record.mode as AppearanceMode)
      : "system",
    chatFontSize:
      typeof record.chatFontSize === "number" &&
      Number.isFinite(record.chatFontSize)
        ? Math.max(12, Math.min(24, Math.round(record.chatFontSize)))
        : 14,
  };
}

export function resolveAppearance(
  mode: AppearanceMode,
  systemDark: boolean,
): "light" | "dark" {
  return mode === "system" ? (systemDark ? "dark" : "light") : mode;
}

function readPreferences(): AppearancePreferences {
  try {
    return normalizeAppearance(
      JSON.parse(localStorage.getItem(APPEARANCE_KEY) || "null"),
    );
  } catch {
    return defaults;
  }
}

export function getAppearance() {
  return current;
}
export function subscribeAppearance(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function saveAppearance(value: AppearancePreferences) {
  const next = normalizeAppearance(value);
  // 先保存成功再发布，避免界面显示的设置与下次启动不一致。
  localStorage.setItem(APPEARANCE_KEY, JSON.stringify(next));
  current = next;
  listeners.forEach((listener) => listener());
}

export function initializeAppearance(
  onResolved?: (theme: "light" | "dark") => void,
) {
  current = readPreferences();
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const apply = () => {
    const theme = resolveAppearance(current.mode, media.matches);
    document.documentElement.dataset.theme = theme;
    document.documentElement.style.colorScheme = theme;
    document.documentElement.style.setProperty(
      "--chat-font-size",
      `${current.chatFontSize}px`,
    );
    onResolved?.(theme);
  };
  const onStorage = (event: StorageEvent) => {
    if (event.key === APPEARANCE_KEY || event.key === null) {
      current = readPreferences();
      listeners.forEach((listener) => listener());
    }
  };
  const unsubscribe = subscribeAppearance(apply);
  media.addEventListener("change", apply);
  window.addEventListener("storage", onStorage);
  apply();
  return () => {
    unsubscribe();
    media.removeEventListener("change", apply);
    window.removeEventListener("storage", onStorage);
  };
}
