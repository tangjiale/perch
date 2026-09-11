import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { RefreshCw } from "lucide-react";
import GlassSelect from "../components/GlassSelect";
import { native } from "../lib/api";
import {
  dingCalendarApi,
  dingCalendarSyncSummary,
  normalizeDingCalendarUrl,
  type DingCalendarConfig,
  type DingCalendarState,
} from "../lib/ding-calendar";
import "./ding-calendar-settings.css";

const intervals = [
  { value: "0", label: "关闭自动同步" },
  { value: "5", label: "每 5 分钟" },
  { value: "15", label: "每 15 分钟" },
  { value: "30", label: "每 30 分钟" },
  { value: "60", label: "每小时" },
];

export default function DingCalendarSettings() {
  const [state, setState] = useState<DingCalendarState | null>(null);
  const [form, setForm] = useState<DingCalendarConfig | null>(null);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const dirty = useRef(false);
  const inFlight = useRef(false);
  const mounted = useRef(false);
  const requestVersion = useRef(0);

  useEffect(() => {
    mounted.current = true;
    if (!native) return () => { mounted.current = false; };
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    async function reload() {
      if (inFlight.current) return;
      const version = ++requestVersion.current;
      try {
        const value = await dingCalendarApi.state();
        if (cancelled || version !== requestVersion.current) return;
        setState(value);
        // 后台刷新只更新连接状态，不能丢失用户的未保存输入。
        if (!dirty.current) setForm(value.config);
        setError("");
      } catch (cause) {
        if (!cancelled && version === requestVersion.current)
          setError(cause instanceof Error ? cause.message : "无法读取日历连接");
      }
    }
    void reload();
    void listen("dingtalk-calendar-updated", () => { void reload(); })
      .then((stop) => { if (cancelled) stop(); else unlisten = stop; })
      .catch(() => { if (!cancelled) setError("日历状态监听失败，可重新打开此设置页刷新"); });
    return () => { cancelled = true; mounted.current = false; unlisten?.(); };
  }, []);

  function update(patch: Partial<DingCalendarConfig>) {
    dirty.current = true;
    setForm((value) => value ? { ...value, ...patch } : value);
    setMessage("");
  }

  async function save(sync: boolean) {
    if (!form || !state || inFlight.current) return;
    inFlight.current = true;
    ++requestVersion.current;
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const serverUrl = normalizeDingCalendarUrl(form.serverUrl);
      const username = form.username.trim();
      if (!username) throw new Error("请填写钉钉提供的 CalDAV 用户名");
      const identityChanged = serverUrl !== (state.config.serverUrl ? normalizeDingCalendarUrl(state.config.serverUrl) : "") || username !== state.config.username;
      if ((!state.config.hasCredential || identityChanged) && !password)
        throw new Error(identityChanged && state.config.hasCredential ? "更换服务器或用户名后，请重新输入 CalDAV 专用密码" : "请填写钉钉提供的 CalDAV 专用密码");
      const needsDiscovery = identityChanged || !form.calendarUrl;
      let value = await dingCalendarApi.save({
        ...form,
        serverUrl,
        username,
        todoCalendarUrl: "",
        todoCalendarName: "",
        ...(needsDiscovery ? { calendarUrl: "", calendarName: "" } : {}),
      }, password || undefined);
      if (!mounted.current) return;
      dirty.current = false;
      setState(value);
      setForm(value.config);
      setPassword("");
      setMessage("连接设置已保存");
      if (sync) {
        value = await dingCalendarApi.sync();
        if (!mounted.current) return;
        setState(value);
        setForm(value.config);
        setMessage(dingCalendarSyncSummary(value));
      }
    } catch (cause) {
      if (mounted.current) setError(cause instanceof Error ? cause.message : "日历操作失败，请重试");
    } finally {
      inFlight.current = false;
      if (mounted.current) setBusy(false);
    }
  }

  async function manualSync() {
    if (inFlight.current) return;
    if (dirty.current || !state?.config.hasCredential) { await save(true); return; }
    inFlight.current = true; ++requestVersion.current;
    setBusy(true); setError(""); setMessage("");
    try {
      const value = await dingCalendarApi.sync();
      if (!mounted.current) return;
      setState(value); setForm(value.config);
      setMessage(dingCalendarSyncSummary(value));
    } catch (cause) {
      if (mounted.current) setError(cause instanceof Error ? cause.message : "日历同步失败");
    } finally {
      inFlight.current = false;
      if (mounted.current) setBusy(false);
    }
  }

  return (
    <section className="ding-calendar-settings" aria-label="钉钉日历设置">
      <div className="section-heading"><h2>钉钉日历</h2></div>
      <p className="ding-calendar-help">使用钉钉提供的 CalDAV 服务器、用户名和专用密码，将日程只读显示在栖点日历中。请在钉钉端修改日程。获取入口：钉钉日历 → 设置 → 同步 → 同步到其他日历 → 获取 CalDAV 账号。</p>
      {!native ? <p className="ding-calendar-help">待连接。请在桌面应用中配置和同步钉钉日历，浏览器预览不保存连接信息。</p> : <>
        {error && <p className="error" role="alert">{error}</p>}
        {!form ? <p role="status">{error ? "连接信息未加载，请重新打开此设置页。" : "正在读取连接设置…"}</p> : <form onSubmit={(event) => { event.preventDefault(); void save(false); }}>
          <fieldset disabled={busy} className="ding-calendar-fields">
            <label htmlFor="ding-server"><span>CalDAV 服务器<span className="required-mark" aria-hidden="true">*</span></span>
              <input id="ding-server" value={form.serverUrl} onChange={(event) => update({ serverUrl: event.target.value })} placeholder="填写钉钉提供的服务器地址" autoComplete="url" required aria-describedby="ding-server-help" />
            </label>
            <p id="ding-server-help" className="ding-calendar-help">可直接填写主机名，保存时自动补充 https://。</p>
            <label htmlFor="ding-username"><span>CalDAV 用户名<span className="required-mark" aria-hidden="true">*</span></span>
              <input id="ding-username" value={form.username} onChange={(event) => update({ username: event.target.value })} autoComplete="username" required />
            </label>
            <label htmlFor="ding-password"><span>CalDAV 专用密码{(!state?.config.hasCredential || form.serverUrl !== state.config.serverUrl || form.username !== state.config.username) && <span className="required-mark" aria-hidden="true">*</span>}</span>
              <input id="ding-password" type="password" value={password} onChange={(event) => { dirty.current = true; setPassword(event.target.value); setMessage(""); }} autoComplete="new-password" placeholder={state?.config.hasCredential ? "已保存密码，留空保留" : "填写钉钉提供的专用密码"} aria-describedby="ding-password-help" />
            </label>
            <p id="ding-password-help" className="ding-calendar-help">密码保存在系统钥匙串。更换服务器或用户名时，需重新输入专用密码。</p>
            <label className="ding-calendar-check"><input type="checkbox" checked={form.enabled} onChange={(event) => update({ enabled: event.target.checked })} />启用钉钉日历同步</label>
            <label htmlFor="ding-interval">自动同步
              <GlassSelect id="ding-interval" disabled={busy} value={String(form.syncIntervalMinutes)} options={intervals.some((item) => item.value === String(form.syncIntervalMinutes)) ? intervals : [...intervals, { value: String(form.syncIntervalMinutes), label: `每 ${form.syncIntervalMinutes} 分钟` }]} onValueChange={(value) => update({ syncIntervalMinutes: Number(value) })} />
            </label>
            <div className="ding-calendar-selection">
              <h3>要显示的日历</h3>
              <p className="ding-calendar-help">首次填写账号后直接点击“保存并获取日历”，无需先关闭同步开关。获取成功后选择“我的日历”，再保存并同步；自动同步会在完成日历选择后生效。</p>
              {state?.calendars.length ? <GlassSelect aria-label="钉钉日历选择" disabled={busy} value={form.calendarUrl} options={[{ value: "", label: "请选择“我的日历”" }, ...state.calendars.filter(calendar => !calendar.components?.length || calendar.components.includes("VEVENT")).map(calendar => ({ value: calendar.url, label: calendar.name || "未命名日历" }))]} onValueChange={calendarUrl => update({ calendarUrl, calendarName: state.calendars.find(calendar => calendar.url === calendarUrl)?.name || "" })} /> : <p className="ding-calendar-help">尚未发现日历，请先保存并获取日历。</p>}
            </div>
            <div className="ding-calendar-actions">
              <button type="submit" className="button">保存设置</button>
              <button type="button" className="button primary" onClick={() => { void save(true); }}><RefreshCw size={15} />{busy ? "处理中…" : (form.calendarUrl || form.todoCalendarUrl) ? "保存并同步" : "保存并获取日历"}</button>
              <button type="button" className="button" title="立即同步；有未保存修改时先保存当前设置" onClick={() => { void manualSync(); }}><RefreshCw size={15} />手动同步</button>
            </div>
          </fieldset>
          <p className="ding-calendar-help">手动同步可随时发起；有未保存修改时会先保存，尚未选择日历时会先获取日历列表。执行期间会暂时禁用按钮，避免重复请求。</p>
          {message && <p role="status">{message}</p>}
          {state && <p className="ding-calendar-help">连接状态：{!state.config.enabled ? "已停用" : !state.config.calendarUrl ? "已启用，待选择日历" : "已启用"}</p>}
          {state?.lastSync && <p className="ding-calendar-help">当前缓存：日程 {state.events.length} 条{state.lastError ? "（本次同步失败，保留上次数据）" : ""}</p>}
          {state?.rangeStart && state.rangeEnd && <p className="ding-calendar-help">已同步范围：{new Date(state.rangeStart).toLocaleDateString("zh-CN")} – {new Date(state.rangeEnd).toLocaleDateString("zh-CN")}</p>}
          {state?.lastSync && <p className="ding-calendar-help">最近成功同步：{new Date(state.lastSync).toLocaleString("zh-CN")}</p>}
          {state?.lastError && <p className="error" role="alert">最近同步失败：{state.lastError}</p>}
        </form>}
      </>}
    </section>
  );
}
