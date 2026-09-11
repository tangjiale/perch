import { afterEach, describe, expect, it, vi } from "vitest";
import {
  APPEARANCE_KEY,
  getAppearance,
  initializeAppearance,
  normalizeAppearance,
  resolveAppearance,
  saveAppearance,
} from "./appearance";

afterEach(() => vi.unstubAllGlobals());

describe("外观设置", () => {
  it("损坏字段恢复默认，字号限制到可读范围", () => {
    expect(normalizeAppearance(null)).toEqual({
      mode: "system",
      chatFontSize: 14,
    });
    expect(normalizeAppearance({ mode: "unknown", chatFontSize: NaN })).toEqual(
      { mode: "system", chatFontSize: 14 },
    );
    expect(normalizeAppearance({ mode: "dark", chatFontSize: 99 })).toEqual({
      mode: "dark",
      chatFontSize: 24,
    });
    expect(normalizeAppearance({ chatFontSize: 0 }).chatFontSize).toBe(12);
  });
  it("固定外观不受系统影响", () => {
    expect(resolveAppearance("light", true)).toBe("light");
    expect(resolveAppearance("dark", false)).toBe("dark");
    expect(resolveAppearance("system", true)).toBe("dark");
    expect(resolveAppearance("system", false)).toBe("light");
  });
  it("启动读取、系统变化与重启保留偏好", () => {
    const records = new Map<string, string>();
    records.set(
      APPEARANCE_KEY,
      JSON.stringify({ mode: "system", chatFontSize: 18 }),
    );
    const media = {
      matches: true,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    };
    const root = {
      dataset: {} as Record<string, string>,
      style: { colorScheme: "", setProperty: vi.fn() },
    };
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => records.get(key),
      setItem: (key: string, value: string) => records.set(key, value),
    });
    vi.stubGlobal("window", {
      matchMedia: () => media,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal("document", { documentElement: root });
    const dispose = initializeAppearance();
    expect(root.dataset.theme).toBe("dark");
    expect(root.style.setProperty).toHaveBeenLastCalledWith(
      "--chat-font-size",
      "18px",
    );
    media.matches = false;
    media.addEventListener.mock.calls[0][1]();
    expect(root.dataset.theme).toBe("light");
    saveAppearance({ mode: "dark", chatFontSize: 20 });
    media.addEventListener.mock.calls[0][1]();
    expect(root.dataset.theme).toBe("dark");
    dispose();
    const cleanup = initializeAppearance();
    expect(getAppearance()).toEqual({ mode: "dark", chatFontSize: 20 });
    expect(root.style.colorScheme).toBe("dark");
    cleanup();
    expect(media.removeEventListener).toHaveBeenCalledTimes(2);
  });
  it("写入失败时保留原设置", () => {
    const before = getAppearance();
    vi.stubGlobal("localStorage", {
      setItem: () => {
        throw Error("full");
      },
    });
    expect(() => saveAppearance({ mode: "light", chatFontSize: 12 })).toThrow(
      "full",
    );
    expect(getAppearance()).toBe(before);
  });
});
