import { useEffect, useState, useSyncExternalStore } from "react";
import { Sun, Moon, Monitor } from "lucide-react";
import {
  getAppearance,
  saveAppearance,
  subscribeAppearance,
  type AppearanceMode,
} from "../lib/appearance";
import "./general-settings.css";
import GlassSelect from "../components/GlassSelect";
import { command, native } from "../lib/api";

interface TrayPreferences {
  closeBehavior: "hide" | "close";
  trayAvailable: boolean;
  warning?: string;
}

const modes = [
  { value: "light", label: "浅色", Icon: Sun },
  { value: "dark", label: "深色", Icon: Moon },
  { value: "system", label: "跟随系统", Icon: Monitor },
] as const;

export default function GeneralSettings() {
  const preferences = useSyncExternalStore(subscribeAppearance, getAppearance);
  const [error, setError] = useState("");
  const [fontInput, setFontInput] = useState(String(preferences.chatFontSize));
  const [tray, setTray] = useState<TrayPreferences | null>(null);
  const [trayBusy, setTrayBusy] = useState(false);
  const [trayError, setTrayError] = useState("");
  useEffect(() => {
    if (!native) return;
    let mounted = true;
    command<TrayPreferences>("tray_preferences")
      .then((value) => {
        if (mounted) setTray(value);
      })
      .catch((error: Error) => {
        if (mounted) setTrayError(error.message);
      });
    return () => {
      mounted = false;
    };
  }, []);
  async function saveCloseBehavior(closeBehavior: string) {
    if (trayBusy) return;
    setTrayBusy(true);
    setTrayError("");
    try {
      setTray(
        await command<TrayPreferences>("tray_preferences_save", {
          closeBehavior,
        }),
      );
    } catch (error) {
      setTrayError(error instanceof Error ? error.message : "无法保存关闭行为");
    } finally {
      setTrayBusy(false);
    }
  }
  useEffect(
    () => setFontInput(String(preferences.chatFontSize)),
    [preferences.chatFontSize],
  );
  function update(value: Partial<typeof preferences>) {
    try {
      saveAppearance({ ...preferences, ...value });
      setError("");
    } catch {
      setError("设置未能保存，请检查本机可用存储空间后重试。");
    }
  }
  return (
    <section className="general-settings">
      <h2>通用设置</h2>
      <fieldset className="appearance-field">
        <legend>外观</legend>
        <div className="appearance-options">
          {modes.map(({ value, label, Icon }) => (
            <label key={value} className="appearance-choice">
              <input
                type="radio"
                name="appearance"
                value={value}
                checked={preferences.mode === value}
                onChange={() => update({ mode: value as AppearanceMode })}
              />
              <span className="appearance-choice-surface">
                <Icon size={19} />
                <span>{label}</span>
              </span>
            </label>
          ))}
        </div>
      </fieldset>
      <div className="general-setting-row">
        <div>
          <label htmlFor="chat-font-size">字号大小</label>
          <p>仅影响会话内容的字号</p>
        </div>
        <div className="font-size-control">
          <input
            id="chat-font-size"
            aria-label="会话字号"
            type="number"
            min={12}
            max={24}
            step={1}
            value={fontInput}
            onChange={(event) => {
              setFontInput(event.target.value);
              const size = event.target.valueAsNumber;
              if (Number.isInteger(size) && size >= 12 && size <= 24)
                update({ chatFontSize: size });
            }}
            onBlur={() => {
              const size = fontInput.trim()
                ? Number(fontInput)
                : preferences.chatFontSize;
              const next = Number.isFinite(size)
                ? Math.max(12, Math.min(24, Math.round(size)))
                : preferences.chatFontSize;
              update({ chatFontSize: next });
              setFontInput(String(next));
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
            }}
          />
          <span>px</span>
        </div>
      </div>
      <div className="general-setting-row close-behavior-row">
        <div>
          <label htmlFor="window-close-behavior">关闭主窗口时</label>
          <p>
            默认保留后台运行，继续接收新邮件。macOS 显示在顶部菜单栏，Windows
            显示在系统托盘。
          </p>
        </div>
        <GlassSelect
          id="window-close-behavior"
          aria-label="关闭主窗口时"
          className="close-behavior-select"
          value={tray?.closeBehavior ?? "hide"}
          disabled={!native || !tray || trayBusy}
          options={[
            { value: "hide", label: "最小化到菜单栏 / 托盘" },
            { value: "close", label: "关闭窗口并退出应用" },
          ]}
          onValueChange={(value) => {
            void saveCloseBehavior(value);
          }}
        />
      </div>
      <p className="general-setting-note">
        将鼠标停留在栖点托盘图标上，可查看今日待办、正在做和逾期任务摘要；点击图标恢复窗口，右键可打开任务或退出栖点。
      </p>
      {!native && (
        <p className="general-setting-note">
          关闭行为和系统托盘需要在桌面应用中设置。
        </p>
      )}
      {tray?.warning && (
        <p role="status" className="error">
          {tray.warning}
        </p>
      )}
      {trayError && (
        <p role="alert" className="error">
          {trayError}
        </p>
      )}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
    </section>
  );
}
