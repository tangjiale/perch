import { useEffect, useRef, useState, type FormEvent } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft,
  Copy,
  Download,
  ExternalLink,
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
} from "lucide-react";
import ExchangeRateTool from "./ExchangeRateTool";
import Modal from "../components/Modal";
import { api, native } from "../lib/api";
import type {
  StorageInfo,
  NatappState,
  NatappTunnel,
  NatappLogs,
  NatappSaveInput,
} from "../lib/types";
import { generateTotp, parseTotp, totpTiming } from "../lib/totp";
import "./tools.css";

const errorText = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

function isWebAddress(address: string) {
  try {
    return ["http:", "https:"].includes(new URL(address).protocol);
  } catch {
    return false;
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
      <header className="tools-heading">
        <div>
          {kind && (
            <button
              className="icon-button"
              title="返回工具列表"
              onClick={() => navigate("/tools")}
            >
              <ArrowLeft size={18} />
            </button>
          )}
          <div>
            <h1>
              {kind === "2fa"
                ? "2FA 验证码"
                : kind === "natapp"
                  ? "NATAPP 内网穿透"
                  : kind === "exchange"
                    ? "汇率转换"
                  : "工具"}
            </h1>
            <p>
              {kind === "2fa"
                ? "在本机生成动态验证码"
                : kind === "natapp"
                  ? "管理本机隧道与运行日志"
                  : kind === "exchange"
                    ? "输入一个金额，同时换算多种货币"
                  : "常用小工具，随时打开使用"}
            </p>
            {kind === "natapp" && (
              <a
                className="tool-website"
                href="https://natapp.cn/"
                target="_blank"
                rel="noopener noreferrer"
                onClick={(event) => {
                  if (!native) return;
                  event.preventDefault();
                  void openUrl("https://natapp.cn/").catch((error) =>
                    notify(`打开官网失败：${errorText(error)}`),
                  );
                }}
              >
                官网：https://natapp.cn/
                <ExternalLink size={13} aria-hidden="true" />
              </a>
            )}
          </div>
        </div>
      </header>
      {kind === "2fa" ? (
        <TotpTool notify={notify} />
      ) : kind === "natapp" ? (
        <NatappTool info={info} notify={notify} />
      ) : kind === "exchange" ? (
        <ExchangeRateTool key={`${info.workspaceId}:${info.dataRoot}`} info={info} notify={notify} />
      ) : (
        <div className="tool-grid">
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
  const [busy, setBusy] = useState("");
  const [form, setForm] = useState<{
    tunnel?: NatappTunnel;
    name: string;
    authToken: string;
  } | null>(null);
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
  const logBox = useRef<HTMLPreElement>(null);
  const logGeneration = useRef(0);
  const logRequest = useRef(0);
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
  useEffect(() => {
    mounted.current = true;
    if (native) void refresh();
    const timer = window.setInterval(() => {
      if (native) void refresh();
    }, 1500);
    return () => {
      mounted.current = false;
      window.clearInterval(timer);
    };
  }, []);
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
        if (key === "install") {
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
    if (!form || busy) return;
    const value: NatappSaveInput = {
      id: form.tunnel?.id,
      revision: form.tunnel?.revision,
      name: form.name.trim(),
      ...(form.authToken.trim() ? { authToken: form.authToken.trim() } : {}),
    };
    void run(
      "save",
      async () => {
        const next = await api.natappSave(value);
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
  return (
    <>
      <div className="tool-panel natapp-install">
        <div>
          <h2>
            {state?.installed
              ? "客户端已安装"
              : state?.installing
                ? "正在安装客户端"
                : "安装 NATAPP 客户端"}
          </h2>
          <p className="tool-hint">
            {state
              ? `${state.platform}${state.version ? ` · ${state.version}` : ""}`
              : "正在检测平台…"}
          </p>
          <p className="tool-data-path" title={state?.dataDir || info.dataRoot}>
            {state?.dataDir || info.dataRoot}
          </p>
        </div>
        <div className="tool-actions">
          <button
            disabled={Boolean(busy) || state?.installing}
            onClick={() => void refresh()}
            title="刷新安装状态"
          >
            <RefreshCw size={15} />
            刷新
          </button>
          {!state?.installed && (
            <button
              className="primary"
              disabled={
                !state ||
                !state.tunnels.some((tunnel) => tunnel.hasAuthToken) ||
                Boolean(busy) ||
                state.installing
              }
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
        </div>
        {(state?.installing || busy === "install") && (
          <div className="natapp-download">
            <progress max={100} value={percent} />
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
                每个隧道使用独立 authToken，启动后可查看实时输出。
              </p>
            </div>
            <button
              className="primary"
              disabled={Boolean(busy)}
              onClick={() => setForm({ name: "", authToken: "" })}
            >
              <Plus size={16} />
              添加隧道
            </button>
          </div>
          {!state.installed && (
            <p className="tool-hint">
              先添加隧道并填写 authToken，再下载并安装客户端。
            </p>
          )}
          {state.tunnels.length === 0 ? (
            <div className="tool-empty">
              <Network size={30} />
              <p>还没有隧道</p>
              <span>添加一个隧道，填写 NATAPP 的 authToken 后启动。</span>
            </div>
          ) : (
            <div className="natapp-list">
              {state.tunnels.map((tunnel) => (
                <article className="natapp-row" key={tunnel.id}>
                  <div className="natapp-row-name">
                    <strong>{tunnel.name}</strong>
                    <span className={`natapp-status ${tunnel.status}`}>
                      {names[tunnel.status]}
                    </span>
                    {tunnel.pid && <small>PID {tunnel.pid}</small>}
                    {(tunnel.forwardingUrl?.trim() ||
                      tunnel.localAddress?.trim()) && (
                      <dl className="natapp-addresses">
                        {tunnel.forwardingUrl?.trim() && (
                          <div>
                            <dt>访问地址</dt>
                            <dd>
                              {isWebAddress(tunnel.forwardingUrl) ? (
                                <a
                                  className="natapp-address-link"
                                  href={tunnel.forwardingUrl.trim()}
                                  target="_blank"
                                  rel="noopener noreferrer"
                                  title="在浏览器中打开访问地址"
                                  onClick={(event) => {
                                    if (!native) return;
                                    event.preventDefault();
                                    void openUrl(tunnel.forwardingUrl!.trim()).catch((error) =>
                                      notify(`打开访问地址失败：${errorText(error)}`),
                                    );
                                  }}
                                >
                                  {tunnel.forwardingUrl}
                                </a>
                              ) : tunnel.forwardingUrl}
                              <button
                                type="button"
                                className="icon-button natapp-copy-address"
                                title="复制访问地址"
                                aria-label="复制访问地址"
                                onClick={async () => {
                                  try {
                                    await navigator.clipboard.writeText(
                                      tunnel.forwardingUrl!,
                                    );
                                    notify("访问地址已复制");
                                  } catch (error) {
                                    notify(`复制失败：${errorText(error)}`);
                                  }
                                }}
                              >
                                <Copy size={14} aria-hidden="true" />
                              </button>
                            </dd>
                          </div>
                        )}
                        {tunnel.localAddress?.trim() && (
                          <div>
                            <dt>本地映射</dt>
                            <dd>{tunnel.localAddress}</dd>
                          </div>
                        )}
                      </dl>
                    )}
                    {tunnel.lastError && (
                      <p className="tool-error">{tunnel.lastError}</p>
                    )}
                  </div>
                  <div className="tool-actions">
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
                    <button
                      className="icon-button"
                      title="编辑隧道"
                      disabled={
                        Boolean(busy) ||
                        tunnel.status === "running" ||
                        tunnel.status === "starting"
                      }
                      onClick={() =>
                        setForm({ tunnel, name: tunnel.name, authToken: "" })
                      }
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
                      <Trash2 size={15} />
                    </button>
                  </div>
                </article>
              ))}
            </div>
          )}
        </div>
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
                disabled={Boolean(busy) || !form.name.trim()}
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
