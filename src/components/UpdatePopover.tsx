import { useEffect, useRef, useState } from "react";
import * as Popover from "@radix-ui/react-popover";
import {
  ArrowDownToLine,
  CheckCircle2,
  ExternalLink,
  LoaderCircle,
  RefreshCw,
  RotateCcw,
  TriangleAlert,
} from "lucide-react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { openUrl } from "@tauri-apps/plugin-opener";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { command, native } from "../lib/api";
import "./update-popover.css";

type Phase =
  | "idle"
  | "checking"
  | "current"
  | "available"
  | "downloading"
  | "installing"
  | "ready"
  | "error";
interface UpdaterStatus {
  configured: boolean;
  releasesUrl: string | null;
}

function errorDetail(error: unknown) {
  const value =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : JSON.stringify(error);
  return value?.replace(/\s+/g, " ").trim().slice(0, 240) || "未知错误";
}

export default function UpdatePopover({ version }: { version: string }) {
  const [open, setOpen] = useState(false);
  const [phase, setPhase] = useState<Phase>("idle");
  const [status, setStatus] = useState<UpdaterStatus | null>(null);
  const [latest, setLatest] = useState<{
    version: string;
    body?: string;
  } | null>(null);
  const [error, setError] = useState("");
  const [received, setReceived] = useState(0);
  const [total, setTotal] = useState<number | undefined>();
  const update = useRef<Update | null>(null);
  const busy = useRef(false);
  const mounted = useRef(true);
  const generation = useRef(0);
  const started = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      generation.current += 1;
      // 下载持有原生资源，等操作结束后再释放。
      if (!busy.current) {
        void update.current?.close().catch(() => undefined);
        update.current = null;
      }
    };
  }, []);

  async function releaseUpdate() {
    const previous = update.current;
    update.current = null;
    await previous?.close().catch(() => undefined);
  }

  async function checkUpdate() {
    if (busy.current || phase === "ready") return;
    if (!native) return;
    busy.current = true;
    const request = ++generation.current;
    const active = () => mounted.current && generation.current === request;
    setPhase("checking");
    setError("");
    setLatest(null);
    try {
      await releaseUpdate();
      const nextStatus = await command<UpdaterStatus>("updater_status");
      if (!active()) return;
      setStatus(nextStatus);
      if (!nextStatus.configured) {
        setPhase("idle");
        return;
      }
      const next = await check({ timeout: 15000, allowDowngrades: false });
      if (!active()) {
        await next?.close().catch(() => undefined);
        return;
      }
      update.current = next;
      setLatest(next ? { version: next.version, body: next.body } : null);
      setPhase(next ? "available" : "current");
    } catch (error) {
      if (active()) {
        setError(`检查更新失败。原因：${errorDetail(error)}`);
        setPhase("error");
      }
    } finally {
      busy.current = false;
      if (!mounted.current) await releaseUpdate();
    }
  }

  useEffect(() => {
    if (!native || started.current) return;
    started.current = true;
    void checkUpdate();
    const timer = window.setInterval(() => {
      void checkUpdate();
    }, 30 * 60 * 1000);
    return () => window.clearInterval(timer);
  }, []);

  async function installUpdate() {
    const next = update.current;
    if (!next || busy.current) return;
    busy.current = true;
    setError("");
    setReceived(0);
    setTotal(undefined);
    setPhase("downloading");
    let prepared = false;
    try {
      await command("update_prepare");
      prepared = true;
      await next.downloadAndInstall(
        (event) => {
          if (!mounted.current) return;
          if (event.event === "Started")
            setTotal(event.data.contentLength || undefined);
          if (event.event === "Progress")
            setReceived((bytes) => bytes + event.data.chunkLength);
          if (event.event === "Finished") setPhase("installing");
        },
        { timeout: 120000 },
      );
      if (mounted.current) setPhase("ready");
      await releaseUpdate();
    } catch (error) {
      if (mounted.current) {
        const prefix = prepared
          ? "更新未完成，请检查网络及磁盘空间后重试。"
          : "暂时不能更新，请等待正在运行的会话结束后重试。";
        setError(`${prefix} 原因：${errorDetail(error)}`);
        setPhase("error");
      }
    } finally {
      busy.current = false;
      if (!mounted.current) await releaseUpdate();
    }
  }

  async function restart() {
    if (busy.current) return;
    busy.current = true;
    setError("");
    try {
      await command("update_prepare");
      await relaunch();
    } catch {
      if (mounted.current)
        setError("暂时无法重启，请等待会话结束，或退出应用后重新打开。");
    } finally {
      busy.current = false;
    }
  }

  async function showRelease() {
    if (!status?.releasesUrl) return;
    try {
      await openUrl(status.releasesUrl);
    } catch {
      setError("无法打开更新日志，请稍后重试。");
    }
  }

  const working = ["checking", "downloading", "installing"].includes(phase);
  const hasUpdate = !!latest;
  const percent = total
    ? Math.min(100, Math.round((received / total) * 100))
    : undefined;
  const message =
    phase === "checking"
      ? "正在检查新版本…"
      : phase === "current"
        ? "已是最新版本"
        : phase === "available"
          ? "有新版本可用"
          : phase === "downloading"
            ? "正在下载更新"
            : phase === "installing"
              ? "正在安装更新，请勿退出"
              : phase === "ready"
                ? "更新已安装，重启后生效"
                : phase === "error"
                  ? "更新暂未完成"
                  : !native
                    ? "请在桌面应用中使用在线更新"
                    : status && !status.configured
                      ? "当前构建尚未配置更新源"
                      : "检查栖点的新版本";

  return (
    <Popover.Root
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
      }}
    >
      <Popover.Trigger asChild>
        <button
          className="update-version-trigger"
          type="button"
          aria-label={`当前版本 v${version}，${hasUpdate ? "有更新，" : ""}检查更新`}
          title="版本与更新"
        >
          v{version}
          {hasUpdate && <i className="update-version-dot" aria-hidden="true" />}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          className="update-popover"
          side="bottom"
          align="start"
          sideOffset={10}
          collisionPadding={14}
          aria-label="版本与更新"
        >
          <header className="update-popover-heading">
            <span>版本与更新</span>
            <button
              type="button"
              className="update-icon-action"
              aria-label="重新检查更新"
              title="重新检查更新"
              disabled={working || phase === "ready" || !native}
              onClick={() => void checkUpdate()}
            >
              <RefreshCw
                size={15}
                className={phase === "checking" ? "update-spinning" : ""}
              />
            </button>
          </header>
          <div className="update-popover-body">
            <div className="update-current-version">
              <strong>v{version}</strong>
              <span>
                当前版本{latest ? ` · 最新版本 v${latest.version}` : ""}
              </span>
            </div>
            <div
              className="update-status"
              data-tone={
                phase === "error"
                  ? "error"
                  : hasUpdate
                    ? "available"
                    : "neutral"
              }
              role="status"
              aria-live="polite"
            >
              {working ? (
                <LoaderCircle size={19} className="update-spinning" />
              ) : phase === "error" ? (
                <TriangleAlert size={19} />
              ) : hasUpdate ? (
                <ArrowDownToLine size={19} />
              ) : phase === "current" ? (
                <CheckCircle2 size={19} />
              ) : (
                <RefreshCw size={19} />
              )}
              <span>
                {message}
                {phase === "available" && <small>v{latest?.version}</small>}
              </span>
            </div>
            {phase === "downloading" && (
              <div className="update-progress">
                <progress aria-label="更新下载进度" max={100} value={percent} />
                <span>
                  {percent === undefined
                    ? `已下载 ${(received / 1024 / 1024).toFixed(1)} MB`
                    : `${percent}% · ${(received / 1024 / 1024).toFixed(1)} / ${((total ?? 0) / 1024 / 1024).toFixed(1)} MB`}
                </span>
              </div>
            )}
            {error && (
              <p className="update-error" role="alert">
                {error}
              </p>
            )}
            {latest?.body && (
              <section className="update-notes" aria-label="本次更新内容">
                <div className="update-notes-title">本次更新内容</div>
                <div className="update-markdown">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>
                    {latest.body}
                  </ReactMarkdown>
                </div>
              </section>
            )}
            {phase === "ready" ? (
              <button
                type="button"
                className="update-primary-action"
                onClick={() => void restart()}
              >
                <RotateCcw size={16} />
                重启应用
              </button>
            ) : hasUpdate && !working ? (
              <button
                type="button"
                className="update-primary-action"
                onClick={() => void installUpdate()}
              >
                <ArrowDownToLine size={16} />
                {phase === "error" ? "重试更新" : "立即更新"}
              </button>
            ) : working ? (
              <button type="button" className="update-primary-action" disabled>
                <LoaderCircle size={16} className="update-spinning" />
                {phase === "checking"
                  ? "正在检查"
                  : phase === "downloading"
                    ? "正在下载"
                    : "正在安装"}
              </button>
            ) : (
              <button
                type="button"
                className="update-primary-action"
                disabled={!native || status?.configured === false}
                onClick={() => void checkUpdate()}
              >
                <RefreshCw size={16} />
                {phase === "error" ? "重新检查" : "检查更新"}
              </button>
            )}
            {status?.releasesUrl && (
              <button
                type="button"
                className="update-release-link"
                onClick={() => void showRelease()}
              >
                查看更新日志
                <ExternalLink size={12} />
              </button>
            )}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
