import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft,
  Activity,
  Copy,
  Download,
  Eye,
  EyeOff,
  ExternalLink,
  FolderOpen,
  KeyRound,
  Network,
  Plus,
  Play,
  Square,
  ScrollText,
  Pencil,
  Trash2,
  RefreshCw,
  ArrowLeftRight,
  WandSparkles,
} from "lucide-react";
import ExchangeRateTool from "./ExchangeRateTool";
import PromptOptimizer from "./PromptOptimizer";
import Modal from "../components/Modal";
import GlassSelect from "../components/GlassSelect";
import { api, native } from "../lib/api";
import type {
  StorageInfo,
  NatappState,
  NatappTunnel,
  NatappLogs,
  NatappSaveInput,
  NatappFreeTunnelInput,
  NatappTunnelTarget,
  NatappRemoteDetails,
} from "../lib/types";
import { generateTotp, parseTotp, totpTiming } from "../lib/totp";
import "./tools.css";

const errorText = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

function natappEndpoint(address: string | null | undefined) {
  if (!address) return null;
  try {
    const url = new URL(address.includes("://") ? address : `tcp://${address}`);
    const web = url.protocol === "http:" || url.protocol === "https:";
    return {
      hostname: url.hostname,
      port: url.port || (web ? (url.protocol === "http:" ? "80" : "443") : null),
      web,
    };
  } catch {
    return null;
  }
}

export default function Tools({
  info,
  notify,
}: {
  info: StorageInfo;
  notify: (text: string) => void;
}) {
  const navigate = useNavigate();
  const pathname = useLocation().pathname;
  const kind = pathname.split("/")[2];
  return (
    <section className="tools-page">
      {kind && (
        <header className={kind === "natapp" ? "tools-heading natapp-tool-heading" : "tools-heading"}>
          <button
            className="icon-button"
            title="返回工具列表"
            onClick={() => navigate("/tools")}
          >
            <ArrowLeft size={18} />
          </button>
          {kind === "natapp" && (
            <>
              <a
                className="tool-website"
                href="https://natapp.cn/download"
                target="_blank"
                rel="noopener noreferrer"
                onClick={(event) => {
                  if (!native) return;
                  event.preventDefault();
                  void openUrl("https://natapp.cn/download").catch((error) =>
                    notify(`打开官网失败：${errorText(error)}`),
                  );
                }}
              >
                NATAPP 官网下载
                <ExternalLink size={13} aria-hidden="true" />
              </a>
              <p className="natapp-setup-hint">
                连接 NATAPP 账号后可在线安装客户端和管理隧道，也可以从文件安装客户端。
              </p>
            </>
          )}
        </header>
      )}
      {kind === "2fa" ? (
        <TotpTool notify={notify} />
      ) : kind === "natapp" ? (
        <NatappTool key={info.workspaceId} info={info} notify={notify} />
      ) : kind === "exchange" ? (
        <ExchangeRateTool key={`${info.workspaceId}:${info.dataRoot}`} info={info} notify={notify} />
      ) : kind === "prompts" ? (
        <PromptOptimizer key={`${info.workspaceId}:${info.dataRoot}`} notify={notify} />
      ) : (
        <div className="tool-grid">
          <button className="tool-card" onClick={() => navigate("/tools/prompts")}>
            <span className="tool-icon"><WandSparkles size={25} /></span>
            <h2>提示词优化</h2>
            <p>优化与迭代提示词，对比效果，管理个人提示词库。</p>
            <span className="tool-card-link">打开工具 →</span>
          </button>
          <button className="tool-card" onClick={() => navigate("/tools/2fa")}>
            <span className="tool-icon">
              <KeyRound size={25} />
            </span>
            <h2>2FA 验证码</h2>
            <p>输入密钥，生成验证码并查看刷新倒计时。</p>
            <span className="tool-card-link">打开工具 →</span>
          </button>
          <button
            className="tool-card"
            onClick={() => navigate("/tools/natapp")}
          >
            <span className="tool-icon">
              <Network size={25} />
            </span>
            <h2>NATAPP 内网穿透</h2>
            <p>安装客户端，管理多个隧道的启停与日志。</p>
            <span className="tool-card-link">打开工具 →</span>
          </button>
          <button className="tool-card" onClick={() => navigate("/tools/exchange")}>
            <span className="tool-icon"><ArrowLeftRight size={25} /></span>
            <h2>汇率转换</h2>
            <p>查询最新参考汇率，一个金额同时换算多种货币。</p>
            <span className="tool-card-link">打开工具 →</span>
          </button>
        </div>
      )}
    </section>
  );
}
function TotpTool({ notify }: { notify: (text: string) => void }) {
  const [input, setInput] = useState("");
  const [code, setCode] = useState("");
  const [error, setError] = useState("");
  const [timing, setTiming] = useState({ remaining: 30, progress: 1 });
  const [details, setDetails] = useState("");
  const generation = useRef(0);
  useEffect(() => {
    const version = ++generation.current;
    setCode("");
    setError("");
    setDetails("");
    if (!input.trim()) return;
    let config;
    try {
      config = parseTotp(input);
    } catch (error) {
      setError(errorText(error));
      return;
    }
    setDetails(
      [
        config.label,
        `${config.digits} 位`,
        config.algorithm.replace("-", ""),
        `${config.period} 秒`,
      ]
        .filter(Boolean)
        .join(" · "),
    );
    let lastCounter = -1;
    let request = 0;
    const update = () => {
      const now = Date.now();
      const next = totpTiming(config.period, now);
      setTiming(next);
      if (next.counter === lastCounter) return;
      lastCounter = next.counter;
      const current = ++request;
      setCode("");
      void generateTotp(config, now)
        .then((value) => {
          if (
            generation.current === version &&
            current === request &&
            totpTiming(config.period).counter === next.counter
          ) {
            setCode(value);
            setError("");
          }
        })
        .catch((error) => {
          if (generation.current === version && current === request)
            setError(errorText(error));
        });
    };
    update();
    const timer = window.setInterval(update, 100);
    document.addEventListener("visibilitychange", update);
    window.addEventListener("focus", update);
    return () => {
      generation.current++;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", update);
      window.removeEventListener("focus", update);
    };
  }, [input]);
  const copy = async () => {
    try {
      // 点击时重新生成，避免在周期边界复制上一周期验证码。
      const value = await generateTotp(parseTotp(input));
      await navigator.clipboard.writeText(value);
      notify("验证码已复制");
    } catch (error) {
      notify(`复制失败：${errorText(error)}`);
    }
  };
  return (
    <div className="tool-panel totp-panel">
      <label className="tool-field">
        2FA 密钥或 otpauth 链接
        <textarea
          value={input}
          onChange={(event) => setInput(event.target.value)}
          placeholder="输入 Base32 密钥或 otpauth://totp/..."
          autoComplete="off"
          autoCorrect="off"
          spellCheck={false}
          rows={3}
        />
      </label>
      <p className="tool-hint">
        密钥仅在当前页面内存中使用，不保存、不发送到网络。离开页面即清空。
      </p>
      {error && (
        <p className="tool-error" role="alert">
          {error}
        </p>
      )}
      <div className="totp-display">
        <span className="tool-hint">动态验证码</span>
        <output aria-label="动态验证码" className="totp-code">
          {code
            ? `${code.slice(0, code.length / 2)} ${code.slice(code.length / 2)}`
            : "— — —"}
        </output>
        <p className="tool-hint">{details || "默认 6 位 · SHA1 · 30 秒"}</p>
        <div
          className={`totp-countdown ${timing.remaining <= 5 && code ? "ending" : ""}`}
        >
          <div
            className="totp-track"
            role="progressbar"
            aria-label="验证码剩余有效时间"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={
              input && !error ? Math.round(timing.progress * 100) : 0
            }
          >
            <span
              style={{
                transform: `scaleX(${input && !error ? timing.progress : 0})`,
              }}
            />
          </div>
          <span>
            {input && !error
              ? `${Math.ceil(timing.remaining)} 秒后刷新`
              : "等待输入密钥"}
          </span>
        </div>
      </div>
      <div className="tool-actions">
        <button className="primary" disabled={!code} onClick={copy}>
          <Copy size={15} />
          复制验证码
        </button>
        <button disabled={!input} onClick={() => setInput("")}>
          清空
        </button>
      </div>
    </div>
  );
}
interface TargetDraft {
  localIp: string;
  port: string;
  inspection?: InspectionDraft;
}

interface InspectionDraft { enabled: boolean; port: string }

const emptyTarget: TargetDraft = { localIp: "", port: "" };
const defaultInspection: InspectionDraft = { enabled: false, port: "4040" };
const ipv4Part = String.raw`(?:25[0-5]|2[0-4]\d|1\d{2}|[1-9]?\d)`;
const ipv4Pattern = `${ipv4Part}(?:\\.${ipv4Part}){3}`;

function NatappTargetFields({ value, disabled, onChange }: {
  value: TargetDraft;
  disabled: boolean;
  onChange: (value: TargetDraft) => void;
}) {
  return (
    <div className="natapp-target-fields">
      <label className="tool-field">
        本地地址
        <input
          required
          disabled={disabled}
          placeholder="例如：127.0.0.1"
          pattern={ipv4Pattern}
          title="请输入 IPv4 地址"
          value={value.localIp}
          onChange={(event) => onChange({ ...value, localIp: event.target.value })}
        />
      </label>
      <label className="tool-field">
        本地端口
        <input
          required
          type="number"
          min={1}
          max={65535}
          step={1}
          placeholder="例如：8080"
          disabled={disabled}
          value={value.port}
          onChange={(event) => onChange({ ...value, port: event.target.value })}
        />
      </label>
    </div>
  );
}

function natappTargetInput(draft: TargetDraft): NatappTunnelTarget {
  return {
    localIp: draft.localIp.trim(),
    localPort: Number(draft.port),
    ...(draft.inspection && { inspection: {
      enabled: draft.inspection.enabled,
      port: draft.inspection.enabled ? Number(draft.inspection.port) : 4040,
    } }),
  };
}

function NatappInspectionFields({ value, disabled, onChange }: {
  value: InspectionDraft; disabled: boolean; onChange: (value: InspectionDraft) => void;
}) {
  return (
    <div className="natapp-inspection-fields">
      <label className="natapp-consent">
        <input type="checkbox" checked={value.enabled} disabled={disabled}
          onChange={(event) => onChange({ ...value, enabled: event.target.checked })} />
        <span>开启本地流量控制台</span>
      </label>
      {value.enabled && (
        <label className="tool-field">
          控制台端口
          <input type="number" min={1} max={65535} step={1} required
            disabled={disabled} value={value.port} placeholder="4040"
            onChange={(event) => onChange({ ...value, port: event.target.value })} />
        </label>
      )}
    </div>
  );
}

function NatappCopyButton({ label, onCopy, notify }: {
  label: string;
  onCopy: () => Promise<void>;
  notify: (text: string) => void;
}) {
  return (
    <button
      type="button"
      className="icon-button natapp-detail-icon"
      title={`复制${label}`}
      aria-label={`复制${label}`}
      onClick={async () => {
        try {
          await onCopy();
          notify(`${label}已复制`);
        } catch {
          notify(`${label}复制失败`);
        }
      }}
    >
      <Copy size={14} aria-hidden="true" />
    </button>
  );
}

function NatappTokenRow({ tunnel, workspaceId, notify }: {
  tunnel: NatappTunnel;
  workspaceId: string;
  notify: (text: string) => void;
}) {
  const [token, setToken] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const request = useRef(0);
  useEffect(() => () => { request.current++; }, []);

  async function toggle() {
    if (token) {
      setToken(null);
      return;
    }
    const current = ++request.current;
    setLoading(true);
    try {
      const revealed = await api.natappRevealToken(workspaceId, tunnel.id, tunnel.revision);
      if (current === request.current) setToken(revealed);
    } catch (error) {
      if (current === request.current) notify(errorText(error));
    } finally {
      if (current === request.current) setLoading(false);
    }
  }

  return (
    <div>
      <dt>Token</dt>
      <dd className="natapp-detail-value">
        <code>{token ?? "••••••••••••"}</code>
        <button
          type="button"
          className="icon-button natapp-detail-icon"
          aria-label={token ? "隐藏隧道 Token" : "显示隧道 Token"}
          title={token ? "隐藏隧道 Token" : "显示隧道 Token"}
          aria-pressed={Boolean(token)}
          disabled={loading}
          onClick={() => void toggle()}
        >
          {token ? <EyeOff size={14} aria-hidden="true" /> : <Eye size={14} aria-hidden="true" />}
        </button>
        <NatappCopyButton
          label="隧道 Token"
          onCopy={() => api.natappCopyToken(workspaceId, tunnel.id, tunnel.revision)}
          notify={notify}
        />
      </dd>
    </div>
  );
}

const natappTrafficNumber = new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 2 });
const natappTrialDate = new Intl.DateTimeFormat("zh-CN", {
  dateStyle: "short", timeStyle: "short", timeZone: "Asia/Shanghai",
});
const natappStatusNames = { online: "官网在线", offline: "官网离线", disabled: "官网禁用" };

function natappTraffic(bytes: number) {
  if (!bytes) return "0 MB";
  const gb = bytes >= 1024 ** 3;
  const size = Math.max(0.01, bytes / (gb ? 1024 ** 3 : 1024 ** 2));
  return `${natappTrafficNumber.format(size)} ${gb ? "GB" : "MB"}`;
}

type NatappDetails = NatappRemoteDetails | null | undefined;

function NatappTrafficDetails({ details }: { details: NatappDetails }) {
  const used = details?.usedBytes;
  const limit = details?.limitBytes;
  return (
    <div>
      <dt>本月流量</dt>
      <dd className="natapp-detail-value">
        {used == null ? "—" : (
          <>
            <span>{natappTraffic(used)}{limit ? ` / ${natappTraffic(limit)}` : ""}</span>
            {limit ? (
              <progress className="natapp-traffic-track" aria-label="本月流量使用比例"
                value={Math.min(limit, used)} max={limit} />
            ) : null}
          </>
        )}
      </dd>
    </div>
  );
}

function NatappDates({ details }: { details: NatappDetails }) {
  const date = details?.expiresOn;
  const today = new Date(Date.now() + 8 * 3600_000).toISOString().slice(0, 10);
  const days = date ? Math.round((Date.parse(date) - Date.parse(today)) / 86_400_000) : null;
  const remaining = days === null ? "" : days < 0 ? "已过期" : days === 0 ? "今天到期" : `剩余 ${days} 天`;
  const trialEnd = details?.trialExpiresAt ? new Date(details.trialExpiresAt * 1000) : null;
  const expires = trialEnd ? natappTrialDate.format(trialEnd) : date || (details?.permanent ? "长期有效" : "—");

  return (
    <>
      <div>
        <dt>创建时间</dt>
        <dd>{details?.createdAt?.replace("T", " ") || "—"}</dd>
      </div>
      <div>
        <dt>到期时间</dt>
        <dd className="natapp-detail-value">
          {expires}
          {remaining && !trialEnd && <span className="muted">{remaining}</span>}
          {trialEnd && <span className="muted">{trialEnd.getTime() <= Date.now() ? "试用已过期" : "试用到期"}</span>}
        </dd>
      </div>
    </>
  );
}

const natappVersion = /^[0-9]+\.[0-9]+\.[0-9]+$/;

function compareNatappVersions(latest: string, installed: string) {
  const currentParts = installed.split(".").map(BigInt);
  const parts = latest.split(".").map(BigInt);
  for (const [index, part] of parts.entries()) {
    if (part !== currentParts[index]) return part > currentParts[index] ? 1 : -1;
  }
  return 0;
}

function NatappTool({
  info,
  notify,
}: {
  info: StorageInfo;
  notify: (text: string) => void;
}) {
  const [state, setState] = useState<NatappState | null>(null);
  const [error, setError] = useState("");
  const [readError, setReadError] = useState("");
  const [busy, setBusy] = useState(native ? "entry" : "");
  const [configured, setConfigured] = useState(false);
  const [clientRelease, setClientRelease] = useState<{
    checking: boolean;
    version?: string;
    error?: string;
  }>({ checking: false });
  const [accountForm, setAccountForm] = useState<{ account: string; password: string; agreed: boolean } | null>(null);
  const [freeForm, setFreeForm] = useState<({ name: string; protocol: NatappFreeTunnelInput["protocol"] } & TargetDraft) | null>(null);
  const [form, setForm] = useState<{
    tunnel?: NatappTunnel;
    name: string;
    authToken: string;
  } | null>(null);
  const [editTarget, setEditTarget] = useState<{ previous: NatappTunnelTarget; draft: TargetDraft } | null>(null);
  const [targetLoading, setTargetLoading] = useState(false);
  const [targetError, setTargetError] = useState("");
  const [deleting, setDeleting] = useState<NatappTunnel | null>(null);
  const [logTunnel, setLogTunnel] = useState<NatappTunnel | null>(null);
  const [logs, setLogs] = useState<NatappLogs | null>(null);
  const [logError, setLogError] = useState("");
  const [logLoading, setLogLoading] = useState(false);
  const mounted = useRef(true);
  const reading = useRef(false);
  const readRequest = useRef(0);
  const operationGeneration = useRef(0);
  const operationBusy = useRef(false);
  const releaseRequest = useRef(0);
  const logBox = useRef<HTMLPreElement>(null);
  const logGeneration = useRef(0);
  const logRequest = useRef(0);
  const editingId = form?.tunnel?.id;
  const editingRevision = form?.tunnel?.revision;
  const clientInstalled = state?.installed === true;
  const checkClientRelease = useCallback(async () => {
    const request = ++releaseRequest.current;
    setClientRelease({ checking: true });
    try {
      const version = await api.natappClientRelease();
      if (!natappVersion.test(version)) throw new Error("官网返回的版本格式无效");
      if (request === releaseRequest.current) setClientRelease({ checking: false, version });
    } catch (error) {
      if (request === releaseRequest.current) {
        setClientRelease({ checking: false, error: errorText(error) });
      }
    }
  }, []);
  useEffect(() => {
    if (!native || !clientInstalled) return;
    void checkClientRelease();
    return () => { releaseRequest.current++; };
  }, [clientInstalled, checkClientRelease]);
  useEffect(() => {
    setEditTarget(null);
    setTargetError("");
    if (!native || !configured || !editingId || editingRevision === undefined) {
      setTargetLoading(false);
      return;
    }
    let cancelled = false;
    setTargetLoading(true);
    void api.natappReadTarget(info.workspaceId, editingId, editingRevision)
      .then((previous) => {
        if (cancelled) return;
        if (!previous) {
          setTargetError("当前 NATAPP 账号未找到这条隧道，请先同步并核对账号。");
          return;
        }
        setEditTarget({
          previous,
          draft: {
            localIp: previous.localIp,
            port: String(previous.localPort),
            inspection: previous.inspection
              ? { enabled: previous.inspection.enabled, port: String(previous.inspection.port) }
              : undefined,
          },
        });
      })
      .catch((error) => { if (!cancelled) setTargetError(errorText(error)); })
      .finally(() => { if (!cancelled) setTargetLoading(false); });
    return () => { cancelled = true; };
  }, [configured, editingId, editingRevision, info.workspaceId]);
  const refresh = async (force = false) => {
    if (reading.current && !force) return;
    reading.current = true;
    const request = ++readRequest.current;
    const generation = operationGeneration.current;
    try {
      const next = await api.natappState();
      if (
        mounted.current &&
        request === readRequest.current &&
        generation === operationGeneration.current
      ) {
        setState(next);
        setReadError("");
      }
    } catch (error) {
      if (
        mounted.current &&
        request === readRequest.current &&
        generation === operationGeneration.current
      )
        setReadError(errorText(error));
    } finally {
      if (request === readRequest.current) reading.current = false;
    }
  };
  const run = async (
    key: string,
    action: () => Promise<unknown>,
    success?: string,
  ) => {
    if (operationBusy.current) return;
    operationBusy.current = true;
    operationGeneration.current++;
    setBusy(key);
    setError("");
    try {
      const result = await action();
      operationGeneration.current++;
      if (!mounted.current) return;
      if (success) notify(success);
      if (
        result &&
        typeof result === "object" &&
        "tunnels" in result &&
        "installed" in result
      ) {
        readRequest.current++;
        reading.current = false;
        setState(result as NatappState);
        setReadError("");
      } else await refresh(true);
    } catch (error) {
      // 失败也结束操作代际，丢弃安装过程中尚未返回的状态读取。
      operationGeneration.current++;
      readRequest.current++;
      reading.current = false;
      if (mounted.current) {
        setError(errorText(error));
        if (key === "install" || key === "update" || key === "install-file") {
          setState((current) =>
            current
              ? {
                  ...current,
                  installing: false,
                  downloadedBytes: 0,
                  totalBytes: undefined,
                }
              : current,
          );
        }
        // 恢复读取与操作提示分离，读取失败也不能让按钮继续停留在安装中。
        void refresh(true);
      }
    } finally {
      operationBusy.current = false;
      if (mounted.current) setBusy("");
    }
  };
  useEffect(() => {
    if (!native) return;
    mounted.current = true;
    let cancelled = false;
    const workspaceId = info.workspaceId;
    void Promise.all([api.natappAccountConfigured(workspaceId), refresh(true)])
      .then(([hasAccount]) => {
        if (cancelled) return;
        setConfigured(hasAccount);
        if (hasAccount) void run("sync", () => api.natappSyncTunnels(workspaceId));
        else setBusy("");
      })
      .catch((error) => {
        if (cancelled) return;
        setError(errorText(error));
        setBusy("");
      });
    const timer = window.setInterval(() => void refresh(), 1500);
    return () => {
      cancelled = true;
      mounted.current = false;
      window.clearInterval(timer);
    };
  }, [info.workspaceId]);
  const readLogs = async (id: string, generation: number) => {
    const request = ++logRequest.current;
    setLogLoading(true);
    try {
      const next = await api.natappLogs(id);
      if (
        !mounted.current ||
        logGeneration.current !== generation ||
        request !== logRequest.current
      )
        return;
      const box = logBox.current;
      const followsTail =
        !box || box.scrollHeight - box.scrollTop - box.clientHeight < 40;
      setLogs(next);
      setLogError("");
      if (followsTail)
        requestAnimationFrame(() => {
          if (logGeneration.current === generation && logBox.current)
            logBox.current.scrollTop = logBox.current.scrollHeight;
        });
    } catch (error) {
      if (
        mounted.current &&
        logGeneration.current === generation &&
        request === logRequest.current
      )
        setLogError(errorText(error));
    } finally {
      if (
        mounted.current &&
        logGeneration.current === generation &&
        request === logRequest.current
      )
        setLogLoading(false);
    }
  };
  useEffect(() => {
    const generation = ++logGeneration.current;
    setLogs(null);
    setLogError("");
    if (!logTunnel) return;
    let pending = false;
    const update = async () => {
      if (pending) return;
      pending = true;
      await readLogs(logTunnel.id, generation);
      pending = false;
    };
    void update();
    const timer = window.setInterval(() => void update(), 2000);
    return () => {
      logGeneration.current++;
      window.clearInterval(timer);
    };
  }, [logTunnel?.id]);
  const save = (event: FormEvent) => {
    event.preventDefault();
    if (!form || busy || targetLoading) return;
    const value: NatappSaveInput = {
      id: form.tunnel?.id,
      revision: form.tunnel?.revision,
      name: form.name.trim(),
      ...(form.authToken.trim() ? { authToken: form.authToken.trim() } : {}),
    };
    const target = editTarget && natappTargetInput(editTarget.draft);
    const targetChanged = editTarget && target &&
      (target.localIp !== editTarget.previous.localIp ||
        target.localPort !== editTarget.previous.localPort ||
        target.inspection?.enabled !== editTarget.previous.inspection?.enabled ||
        (target.inspection?.enabled && target.inspection.port !== editTarget.previous.inspection?.port));
    if (targetChanged && value.authToken) {
      setError("请先保存隧道配置，再单独更换 authToken。");
      return;
    }
    void run(
      "save",
      async () => {
        const next = targetChanged && editTarget && target && form.tunnel
          ? await api.natappConfigureTarget(info.workspaceId, {
              id: form.tunnel.id,
              revision: form.tunnel.revision,
              name: value.name,
              previous: editTarget.previous,
              target,
            })
          : await api.natappSave(value);
        if (mounted.current) setForm(null);
        return next;
      },
      "隧道已保存",
    );
  };
  if (!native)
    return (
      <div className="tool-panel">
        <h2>请在栖点桌面应用中使用</h2>
        <p className="tool-hint">
          NATAPP 的安装、配置及进程管理需要原生桌面环境。2FA 工具可直接使用。
        </p>
      </div>
    );
  const names = {
    stopped: "已停止",
    starting: "启动中",
    running: "运行中",
    error: "运行异常",
  };
  const percent = state?.totalBytes
    ? Math.min(100, (state.downloadedBytes / state.totalBytes) * 100)
    : undefined;
  const installedVersion = state?.version && natappVersion.test(state.version)
    ? state.version
    : undefined;
  const latestVersion = clientRelease.version;
  const versionOrder = installedVersion && latestVersion
    ? compareNatappVersions(latestVersion, installedVersion)
    : undefined;
  const canUpdate = latestVersion && (!installedVersion || versionOrder === 1);
  const tunnelActive = state?.tunnels.some(({ status }) => status === "running" || status === "starting");
  const installingOnline = busy === "install" || busy === "update";
  const fileInstallLabel = clientInstalled ? "从文件更新" : "从文件安装";
  const clientBusy = Boolean(busy) || state?.installing;
  const installDisabled = !state || clientBusy || tunnelActive;
  let versionStatus = latestVersion ? `官网版本 ${latestVersion}` : "";
  if (versionOrder === 1) versionStatus = `发现新版 ${latestVersion}`;
  if (versionOrder === 0) versionStatus = "已是最新版本";
  if (clientRelease.error) versionStatus = `检查更新失败 · ${clientRelease.error}`;
  if (clientRelease.checking) versionStatus = "正在检查更新…";
  return (
    <>
      <div className="tool-panel natapp-install">
        <div className="natapp-client-details">
          <h2>
            {clientInstalled
              ? "客户端已安装"
              : state?.installing
                ? "正在安装客户端"
                : "安装 NATAPP 客户端"}
          </h2>
          <p className="tool-hint">
            {state?.platform || "正在检测平台…"}
            {clientInstalled && ` · ${installedVersion ?? "版本未知"}`}
          </p>
          {state && (
            <p className="tool-data-path" title={state.dataDir}>
              {state.dataDir}
            </p>
          )}
          {clientInstalled && versionStatus && (
            <p className={versionOrder === 1 ? "natapp-release-status available" : "natapp-release-status"} role="status">
              {versionStatus}
            </p>
          )}
        </div>
        <div className="tool-actions">
          <button
            disabled={clientBusy || clientRelease.checking}
            onClick={() => void (clientInstalled ? checkClientRelease() : refresh())}
          >
            <RefreshCw size={15} />
            {clientInstalled ? "检查更新" : "刷新"}
          </button>
          {state && (
            <button
              disabled={installDisabled}
              title={clientInstalled ? "选择安装包，覆盖当前客户端" : undefined}
              onClick={() => {
                void run("install-file", async () => {
                  const path = await open({
                    title: "选择从 NATAPP 官网获取的安装包",
                    filters: state.platform.startsWith("Windows")
                      ? [{ name: "NATAPP ZIP 安装包", extensions: ["zip"] }]
                      : undefined,
                  });
                  if (!mounted.current || typeof path !== "string") return;
                  const result = await api.natappInstall(path);
                  if (mounted.current) notify(clientInstalled ? "客户端更新完成" : "客户端安装完成");
                  return result;
                });
              }}
            >
              <FolderOpen size={15} />
              {busy === "install-file" ? `正在${fileInstallLabel}…` : fileInstallLabel}
            </button>
          )}
          {!clientInstalled && (
            <button
              className="primary"
              disabled={installDisabled || !configured}
              onClick={() =>
                void run("install", () => api.natappInstall(), "客户端安装完成")
              }
            >
              <Download size={15} />
              {state?.installing || busy === "install"
                ? "安装中…"
                : "下载并安装"}
            </button>
          )}
          {clientInstalled && (canUpdate || busy === "update") && (
            <button
              className="primary"
              disabled={installDisabled || !configured}
              onClick={() => void run("update", () => api.natappInstall(undefined, true), "客户端更新完成")}
            >
              <Download size={15} />
              {busy === "update" ? "更新中…" : installedVersion ? "下载并更新" : "安装官网版"}
            </button>
          )}
        </div>
        {state && !configured && busy !== "entry" && (
          <p className="natapp-install-hint">请先连接 NATAPP 账号，再在线安装或更新。</p>
        )}
        {tunnelActive && (
          <p className="natapp-install-hint">请先停止运行中的隧道，再更新或覆盖安装客户端。</p>
        )}
        {(state?.installing || installingOnline) && (
          <div className="natapp-download">
            <progress aria-label="NATAPP 安装进度" max={100} value={percent} />
            <span>
              {formatBytes(state?.downloadedBytes || 0)}
              {state?.totalBytes ? ` / ${formatBytes(state.totalBytes)}` : ""}
              {percent !== undefined
                ? ` · ${Math.round(percent)}%`
                : " · 正在准备或下载"}
            </span>
          </div>
        )}
      </div>
      {readError && (
        <div className="tool-error" role="alert">
          {readError}
          <button onClick={() => void refresh()}>重新加载</button>
        </div>
      )}
      {error && (
        <div className="tool-error" role="alert">
          {error}
          <button onClick={() => void refresh()}>重新加载</button>
        </div>
      )}
      {state && (
        <div className="tool-panel">
          <div className="tool-section-heading">
            <div>
              <h2>穿透列表</h2>
              <p className="tool-hint">
                连接 NATAPP 账号后可同步隧道和创建免费隧道。
              </p>
            </div>
            <div className="tool-actions natapp-heading-actions">
              <button
                disabled={Boolean(busy)}
                onClick={() => void run("console", () => api.natappOpenConsole(info.workspaceId))}
              >
                <ExternalLink size={15} />
                {busy === "console" && !freeForm ? "正在打开…" : "NATAPP 控制台"}
              </button>
              <button
                disabled={Boolean(busy)}
                onClick={() => {
                  setError("");
                  setAccountForm({ account: "", password: "", agreed: false });
                }}
              >
                <KeyRound size={15} />
                {configured ? "更换账号" : "连接账号"}
              </button>
              {configured && (
                <button
                  disabled={Boolean(busy)}
                  onClick={() => void run("sync", () => api.natappSyncTunnels(info.workspaceId), "隧道已同步")}
                >
                  <RefreshCw size={15} />
                  {busy === "sync" ? "同步中…" : "同步隧道"}
                </button>
              )}
              <button
                disabled={Boolean(busy)}
                onClick={() => setForm({ name: "", authToken: "" })}
              >
                手动添加
              </button>
              <button
                className="primary"
                disabled={Boolean(busy)}
                onClick={() => {
                  setError("");
                  setFreeForm({ name: "", protocol: "http", localIp: "127.0.0.1", port: "", inspection: defaultInspection });
                }}
              >
                <Plus size={16} />
                创建隧道
              </button>
            </div>
          </div>
          {state.tunnels.length === 0 ? (
            <div className="tool-empty">
              <Network size={30} />
              <p>还没有隧道</p>
              <span>连接账号同步已有隧道，或创建一条新隧道。</span>
            </div>
          ) : (
            <div className="natapp-list">
              {state.tunnels.map((tunnel) => {
                const details = tunnel.remoteDetails;
                const address = tunnel.forwardingUrl || details?.endpoint;
                const forwarding = natappEndpoint(address);
                const target = tunnel.configuredTarget;
                const local = natappEndpoint(tunnel.localAddress);
                const localAddress = local?.hostname || target?.localIp;
                const localPort = local?.port || target?.localPort;
                return (
                  <article className="natapp-row" key={tunnel.id}>
                    <div className="natapp-row-title">
                      <strong>{tunnel.name}</strong>
                      {details?.protocol && <span className="natapp-status natapp-protocol">{details.protocol}</span>}
                      {details?.plan && <span className="natapp-status">{details.plan}</span>}
                      {details?.status && (
                        <span className={`natapp-status ${details.status === "online" ? "running" : ""}`}
                          title={`同步于 ${new Date(details.syncedAt).toLocaleString("zh-CN")}`}>
                          {natappStatusNames[details.status]}
                        </span>
                      )}
                      <span className={`natapp-status ${tunnel.status}`}>本机{names[tunnel.status]}</span>
                      {tunnel.pid && <small className="muted">PID {tunnel.pid}</small>}
                    </div>
                    <div className="tool-actions natapp-row-actions">
                      {tunnel.status === "running" ||
                      tunnel.status === "starting" ? (
                        <button
                          disabled={Boolean(busy)}
                          onClick={() =>
                            void run(tunnel.id, () => api.natappStop(tunnel.id))
                          }
                        >
                          <Square size={14} className="natapp-stop-icon" />
                          停止
                        </button>
                      ) : (
                        <button
                          disabled={
                            Boolean(busy) ||
                            !state.installed ||
                            !tunnel.hasAuthToken
                          }
                          onClick={() =>
                            void run(tunnel.id, () => api.natappStart(tunnel.id))
                          }
                        >
                          <Play size={14} className="natapp-start-icon" />
                          启动
                        </button>
                      )}
                      <button
                        disabled={Boolean(busy)}
                        onClick={() => setLogTunnel(tunnel)}
                      >
                        <ScrollText size={15} />
                        日志
                      </button>
                      {tunnel.inspectionPort && (
                        <button
                          disabled={Boolean(busy)}
                          title={`打开本地流量控制台 · 端口 ${tunnel.inspectionPort}`}
                          onClick={() => void run("inspection", () => api.natappOpenInspection(info.workspaceId, tunnel.id))}
                        >
                          <Activity size={15} />
                          流量控制台
                        </button>
                      )}
                      <button
                        className="icon-button"
                        title="编辑隧道"
                        disabled={
                          Boolean(busy) ||
                          tunnel.status === "running" ||
                          tunnel.status === "starting"
                        }
                        onClick={() => {
                          setError("");
                          setEditTarget(null);
                          setTargetError("");
                          setTargetLoading(configured);
                          setForm({ tunnel, name: tunnel.name, authToken: "" });
                        }}
                      >
                        <Pencil size={15} />
                      </button>
                      <button
                        className="icon-button"
                        title="删除隧道"
                        disabled={
                          Boolean(busy) ||
                          tunnel.status === "running" ||
                          tunnel.status === "starting"
                        }
                        onClick={() => setDeleting(tunnel)}
                      >
                        <Trash2 className="danger" size={15} />
                      </button>
                    </div>
                    <div className="natapp-row-details">
                      <dl className="natapp-addresses">
                        {tunnel.remoteId && (
                          <div>
                            <dt>隧道 ID</dt>
                            <dd className="natapp-detail-value">
                              <code>{tunnel.remoteId}</code>
                              <NatappCopyButton label="隧道 ID"
                                onCopy={() => navigator.clipboard.writeText(tunnel.remoteId!)} notify={notify} />
                            </dd>
                          </div>
                        )}
                        {tunnel.hasAuthToken && (
                          <NatappTokenRow key={`${info.workspaceId}:${tunnel.id}:${tunnel.revision}`}
                            tunnel={tunnel} workspaceId={info.workspaceId} notify={notify} />
                        )}
                        <NatappTrafficDetails details={details} />
                      </dl>
                      <dl className="natapp-addresses">
                        <div>
                          <dt>转发地址</dt>
                          <dd className="natapp-detail-value">
                            {address ? (
                              <>
                                {forwarding?.web ? (
                                  <a
                                    className="natapp-address-link"
                                    href={address}
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    title="在浏览器中打开转发地址"
                                    onClick={(event) => {
                                      event.preventDefault();
                                      void openUrl(address).catch(() => notify("打开转发地址失败"));
                                    }}
                                  >
                                    {address}
                                  </a>
                                ) : <span>{address}</span>}
                                <NatappCopyButton label="转发地址"
                                  onCopy={() => navigator.clipboard.writeText(address)} notify={notify} />
                              </>
                            ) : "—"}
                          </dd>
                        </div>
                        <div>
                          <dt>转发端口</dt>
                          <dd>{forwarding?.port || "—"}</dd>
                        </div>
                        <div>
                          <dt>本地映射</dt>
                          <dd>{localAddress && localPort ? `${localAddress}:${localPort}` : "—"}</dd>
                        </div>
                      </dl>
                      <dl className="natapp-addresses">
                        <NatappDates details={details} />
                        {target?.inspection && (
                          <div>
                            <dt>控制台</dt>
                            <dd>{target.inspection.enabled ? `已开启 · ${target.inspection.port}` : "已关闭"}</dd>
                          </div>
                        )}
                      </dl>
                    </div>
                    {tunnel.lastError && (
                      <p className="tool-error">{tunnel.lastError}</p>
                    )}
                  </article>
                );
              })}
            </div>
          )}
        </div>
      )}
      {accountForm && (
        <Modal title="连接 NATAPP 账号" onClose={() => setAccountForm(null)} busy={busy === "connect" || busy === "disconnect"}>
          <form className="tool-dialog-form" onSubmit={(event) => {
            event.preventDefault();
            if (!accountForm.agreed) return;
            const { account, password } = accountForm;
            void run("connect", async () => {
              const next = await api.natappSyncTunnels(info.workspaceId, { account, password });
              if (mounted.current) {
                setConfigured(true);
                setAccountForm(null);
              }
              return next;
            }, "NATAPP 账号已连接，隧道已同步");
          }}>
            <label className="tool-field">
              账号
              <input required autoComplete="off" value={accountForm.account} disabled={Boolean(busy)}
                placeholder="手机号码、邮箱或昵称"
                onChange={(event) => setAccountForm({ ...accountForm, account: event.target.value })} />
            </label>
            <label className="tool-field">
              密码
              <input required type="password" autoComplete="new-password" value={accountForm.password} disabled={Boolean(busy)}
                onChange={(event) => setAccountForm({ ...accountForm, password: event.target.value })} />
            </label>
            <label className="natapp-consent">
              <input type="checkbox" required checked={accountForm.agreed} disabled={Boolean(busy)}
                onChange={(event) => setAccountForm({ ...accountForm, agreed: event.target.checked })} />
              <span>我同意 NATAPP 服务协议及隐私政策，并在当前工作空间加密保存账号密码。</span>
            </label>
            <div className="tool-actions">
              {configured && (
                <button type="button" disabled={Boolean(busy)} onClick={() => void run("disconnect", async () => {
                  await api.natappDisconnectAccount(info.workspaceId);
                  if (mounted.current) {
                    setConfigured(false);
                    setAccountForm(null);
                  }
                }, "已断开 NATAPP 账号")}>
                  断开账号
                </button>
              )}
              <button type="button" onClick={() => void run("terms", () => openUrl("https://natapp.cn/article/serviceagreement"))} disabled={Boolean(busy)}>
                查看官网协议
              </button>
              <button className="primary" disabled={Boolean(busy) || !accountForm.agreed}>
                {busy === "connect" ? "登录中…" : "登录并同步"}
              </button>
            </div>
            {error && <p className="tool-error" role="alert">{error}</p>}
          </form>
        </Modal>
      )}
      {freeForm && (
        <Modal title="创建 NATAPP 隧道" onClose={() => setFreeForm(null)} busy={busy === "create"}>
          <form className="tool-dialog-form" onSubmit={(event) => {
            event.preventDefault();
            if (!configured) return;
            const value = {
              name: freeForm.name.trim(),
              protocol: freeForm.protocol,
              ...natappTargetInput({ ...freeForm, inspection: freeForm.protocol === "http" ? freeForm.inspection : undefined }),
            };
            void run("create", async () => {
              const next = await api.natappCreateFreeTunnel(info.workspaceId, value);
              if (mounted.current) setFreeForm(null);
              return next;
            }, "免费隧道已创建并添加到列表");
          }}>
            {!configured && <p className="tool-hint">连接 NATAPP 账号后可在这里创建免费隧道。</p>}
            <label className="tool-field">
              隧道名称
              <input required maxLength={16} disabled={!configured || Boolean(busy)} value={freeForm.name}
                onChange={(event) => setFreeForm({ ...freeForm, name: event.target.value })} />
            </label>
            <label className="tool-field">
              协议
              <GlassSelect aria-label="协议" value={freeForm.protocol} disabled={!configured || Boolean(busy)}
                options={[{ value: "http", label: "Web (HTTP)" }, { value: "tcp", label: "TCP" }, { value: "udp", label: "UDP" }]}
                onValueChange={(protocol) => setFreeForm({ ...freeForm, protocol: protocol as NatappFreeTunnelInput["protocol"] })} />
            </label>
            <NatappTargetFields
              value={freeForm}
              disabled={!configured || Boolean(busy)}
              onChange={(target) => setFreeForm({ ...freeForm, ...target })}
            />
            {freeForm.protocol === "http" && (
              <NatappInspectionFields
                value={freeForm.inspection ?? defaultInspection}
                disabled={!configured || Boolean(busy)}
                onChange={(inspection) => setFreeForm({ ...freeForm, inspection })}
              />
            )}
            <div className="tool-actions">
              <button type="button" disabled={Boolean(busy)} onClick={() => void run("console", () => api.natappOpenConsole(info.workspaceId, true))}>
                {busy === "console" ? "正在打开…" : "查看付费方案"}
              </button>
              <button className="primary" disabled={!configured || Boolean(busy) || !freeForm.name.trim()}>
                {busy === "create" ? "创建中…" : "创建免费隧道"}
              </button>
            </div>
            {error && <p className="tool-error" role="alert">{error}</p>}
          </form>
        </Modal>
      )}
      {form && (
        <Modal
          title={form.tunnel ? "编辑隧道" : "添加隧道"}
          onClose={() => setForm(null)}
          busy={busy === "save"}
        >
          <form className="tool-dialog-form" onSubmit={save}>
            <label className="tool-field">
              名称
              <input
                required
                maxLength={100}
                value={form.name}
                onChange={(event) =>
                  setForm({ ...form, name: event.target.value })
                }
                disabled={Boolean(busy)}
                placeholder="例如：本地开发服务"
              />
            </label>
            <label className="tool-field">
              authToken
              <input
                type="password"
                autoComplete="new-password"
                spellCheck={false}
                required={!form.tunnel?.hasAuthToken}
                value={form.authToken}
                onChange={(event) =>
                  setForm({ ...form, authToken: event.target.value })
                }
                disabled={Boolean(busy)}
                placeholder={
                  form.tunnel?.hasAuthToken
                    ? "已保存，留空保持不变"
                    : "输入 16 位隧道 authToken"
                }
              />
            </label>
            {form.tunnel && (
              <>
                <NatappTargetFields
                  value={editTarget?.draft ?? emptyTarget}
                  disabled={!editTarget || targetLoading || Boolean(busy)}
                  onChange={(draft) => setEditTarget((current) => current && { ...current, draft })}
                />
                {editTarget?.draft.inspection && (
                  <NatappInspectionFields
                    value={editTarget.draft.inspection}
                    disabled={targetLoading || Boolean(busy)}
                    onChange={(inspection) => setEditTarget((current) =>
                      current && { ...current, draft: { ...current.draft, inspection } })}
                  />
                )}
                {targetLoading && <p className="tool-hint" role="status">正在读取 NATAPP 配置…</p>}
                {!configured && <p className="tool-hint">连接对应的 NATAPP 账号后可配置地址和端口。</p>}
                {targetError && <p className="tool-hint" role="alert">{targetError}</p>}
              </>
            )}
            <p className="tool-hint">
              从 NATAPP 官网隧道列表复制 16 位字母或数字的
              authToken，请勿填写账户 API Token。 authToken
              在本机加密保存；运行中的隧道请先停止再编辑。
            </p>
            {error && (
              <p className="tool-error" role="alert">
                {error}
              </p>
            )}
            <div className="tool-actions">
              <button
                type="button"
                disabled={Boolean(busy)}
                onClick={() => setForm(null)}
              >
                取消
              </button>
              <button
                className="primary"
                disabled={Boolean(busy) || targetLoading || !form.name.trim()}
              >
                {busy === "save" ? "保存中…" : "保存"}
              </button>
            </div>
          </form>
        </Modal>
      )}
      {deleting && (
        <Modal
          title="删除隧道"
          onClose={() => setDeleting(null)}
          busy={busy === "delete"}
        >
          <div className="tool-dialog-form">
            <p>确定删除「{deleting.name}」及其本地配置和日志？</p>
            {error && (
              <p className="tool-error" role="alert">
                {error}
              </p>
            )}
            <div className="tool-actions">
              <button
                disabled={Boolean(busy)}
                onClick={() => setDeleting(null)}
              >
                取消
              </button>
              <button
                className="danger"
                disabled={Boolean(busy)}
                onClick={() =>
                  void run(
                    "delete",
                    async () => {
                      const next = await api.natappDelete(
                        deleting.id,
                        deleting.revision,
                      );
                      if (mounted.current) setDeleting(null);
                      return next;
                    },
                    "隧道已删除",
                  )
                }
              >
                确认删除
              </button>
            </div>
          </div>
        </Modal>
      )}
      {logTunnel && (
        <Modal
          title={`${logTunnel.name} · 输出日志`}
          wide
          onClose={() => setLogTunnel(null)}
        >
          <div className="tool-dialog-form">
            <div className="tool-section-heading">
              <p className="tool-hint">每 2 秒刷新；向上滚动时保留当前位置。</p>
              <button
                disabled={logLoading}
                onClick={() =>
                  void readLogs(logTunnel.id, logGeneration.current)
                }
              >
                <RefreshCw size={14} />
                刷新
              </button>
            </div>
            {logError && (
              <p className="tool-error" role="alert">
                {logError}
              </p>
            )}
            {logs?.truncated && (
              <p className="tool-hint">仅展示最近的日志内容。</p>
            )}
            <pre className="natapp-logs" ref={logBox}>
              {logs?.text || (logLoading ? "正在读取日志…" : "暂无输出日志")}
            </pre>
          </div>
        </Modal>
      )}
    </>
  );
}
function formatBytes(value: number) {
  return value < 1024 * 1024
    ? `${(value / 1024).toFixed(1)} KB`
    : `${(value / (1024 * 1024)).toFixed(1)} MB`;
}
