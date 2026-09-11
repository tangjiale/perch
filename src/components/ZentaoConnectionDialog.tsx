import { useEffect, useRef, useState, type FormEvent } from "react";
import { CircleAlert, KeyRound, LogIn, RefreshCw, Save } from "lucide-react";
import { command } from "../lib/api";
import type { Connection } from "../lib/types";
import GlassSelect from "./GlassSelect";
import Modal from "./Modal";

export default function ZentaoConnectionDialog({
  connection,
  workspaceId,
  onClose,
  onSaved,
  inline = false,
  onBusyChange,
}: {
  connection: Connection;
  workspaceId: string;
  onClose: () => void;
  onSaved: () => Promise<void>;
  inline?: boolean;
  onBusyChange?: (busy: boolean) => void;
}) {
  const existing = connection.revision !== undefined;
  const [draft, setDraft] = useState({
    ...connection,
    syncIntervalMinutes: connection.syncIntervalMinutes ?? 5,
  });
  const [mode, setMode] = useState<"account" | "token">(
    connection.authMode ||
      (connection.loginAccount || connection.rememberCredentials
        ? "account"
        : existing
          ? "token"
          : "account"),
  );
  const [account, setAccount] = useState(connection.loginAccount || "");
  const [rememberCredentials, setRememberCredentials] = useState(
    connection.rememberCredentials ?? false,
  );
  const [password, setPassword] = useState("");
  const [token, setToken] = useState("");
  const [allowInsecureHttp, setAllowInsecureHttp] = useState(false);
  const [hasSavedPassword, setHasSavedPassword] = useState(false);
  const [editingPassword, setEditingPassword] = useState(!existing);
  const [preferencesLoading, setPreferencesLoading] = useState(existing);
  const [help, setHelp] = useState(false);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  const inFlight = useRef(false);
  const mounted = useRef(true);
  const form = useRef<HTMLFormElement>(null);
  let insecure = false;
  try {
    insecure = new URL(draft.baseUrl.trim()).protocol === "http:";
  } catch {
    // 地址输入过程中可能尚未形成完整 URL，由表单和后端校验最终值。
  }
  const needsHttpConsent = mode === "account" && insecure;
  const locked = busy || saved || preferencesLoading;
  const retainingPassword =
    hasSavedPassword &&
    !editingPassword &&
    account.trim() === (connection.loginAccount || "");
  const loggingIn = mode === "account" && !retainingPassword;
  const helpButton = (
    <button
      className="icon-button"
      type="button"
      title="登录与令牌说明"
      aria-label="登录与令牌说明"
      aria-expanded={help}
      onClick={() => setHelp(!help)}
    >
      <CircleAlert size={15} />
    </button>
  );

  useEffect(() => {
    mounted.current = true;
    const dialog = form.current?.closest("dialog");
    const preventBusyCancel = (event: Event) => {
      if (inFlight.current) event.preventDefault();
    };
    dialog?.addEventListener("cancel", preventBusyCancel);
    return () => {
      mounted.current = false;
      dialog?.removeEventListener("cancel", preventBusyCancel);
    };
  }, []);

  useEffect(() => {
    if (!existing) return;
    let active = true;
    command<{ hasSavedPassword: boolean; allowInsecureHttp: boolean }>(
      "zentao_auth_preferences",
      { workspaceId, connectionId: connection.id },
    )
      .then((preferences) => {
        if (!active) return;
        setHasSavedPassword(preferences.hasSavedPassword);
        setEditingPassword(!preferences.hasSavedPassword);
        setAllowInsecureHttp(preferences.allowInsecureHttp);
      })
      .catch(() => {
        if (active) {
          setEditingPassword(true);
          setError(
            "无法检查已保存的登录配置，可重新输入密码保存；原密码不会显示在界面中。",
          );
        }
      })
      .finally(() => {
        if (active) setPreferencesLoading(false);
      });
    return () => {
      active = false;
    };
  }, [existing, workspaceId, connection.id]);

  function close() {
    if (inFlight.current) return;
    setPassword("");
    setToken("");
    onClose();
  }

  function changeMode(next: "account" | "token") {
    if (locked) return;
    setMode(next);
    setPassword("");
    setToken("");
    setError("");
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (
      inFlight.current ||
      preferencesLoading ||
      (!saved && needsHttpConsent && !allowInsecureHttp)
    )
      return;
    inFlight.current = true;
    setBusy(true);
    onBusyChange?.(true);
    setError("");
    let persisted = saved;
    try {
      if (!persisted) {
        await command<Connection>("zentao_connect", {
          connection: {
            ...draft,
            revision: connection.revision,
            name: draft.name.trim(),
            baseUrl: draft.baseUrl.trim(),
            authMode: mode,
          },
          workspaceId,
          account: loggingIn ? account.trim() : null,
          password: loggingIn ? password : null,
          token: mode === "token" ? token.trim() || null : null,
          allowInsecureHttp: needsHttpConsent && allowInsecureHttp,
          rememberCredentials:
            mode === "account"
              ? rememberCredentials
              : !!connection.rememberCredentials &&
                !token.trim() &&
                rememberCredentials,
        });
        persisted = true;
        if (mounted.current) {
          setSaved(true);
          setPassword("");
          setToken("");
        }
      }
      await onSaved();
      if (mounted.current) onClose();
    } catch (cause) {
      if (mounted.current) {
        const message = cause instanceof Error ? cause.message : "请稍后重试";
        setError(
          persisted
            ? `连接已保存，但列表刷新失败：${message}。可重试刷新或关闭窗口。`
            : message,
        );
      }
    } finally {
      inFlight.current = false;
      if (mounted.current) setBusy(false);
      onBusyChange?.(false);
    }
  }

  const content = (
    <form ref={form} className="form-grid" onSubmit={submit} aria-busy={busy}>
      <label>
        连接名称
        <input
          required
          disabled={locked}
          value={draft.name}
          onChange={(event) => setDraft({ ...draft, name: event.target.value })}
        />
      </label>
      <label>
        服务地址
        <input
          required
          type="url"
          readOnly={existing}
          disabled={locked}
          placeholder="https://zentao.example.com"
          value={draft.baseUrl}
          onChange={(event) => {
            setDraft({ ...draft, baseUrl: event.target.value });
            setAllowInsecureHttp(false);
          }}
        />
      </label>
      <label>
        API 版本
        <GlassSelect
          aria-label="禅道 API 版本"
          disabled={locked}
          value={draft.apiVersion}
          onValueChange={(apiVersion) =>
            setDraft({ ...draft, apiVersion: apiVersion as "v1" | "v2" })
          }
          options={[
            { value: "v1", label: "REST v1" },
            { value: "v2", label: "REST v2" },
          ]}
        />
      </label>
      <label>
        自动同步频率
        <GlassSelect
          aria-label="禅道自动同步频率"
          disabled={locked}
          value={String(draft.syncIntervalMinutes ?? 5)}
          onValueChange={(value) =>
            setDraft({ ...draft, syncIntervalMinutes: Number(value) })
          }
          options={[
            { value: "0", label: "关闭自动同步" },
            { value: "1", label: "每 1 分钟" },
            { value: "5", label: "每 5 分钟（推荐）" },
            { value: "10", label: "每 10 分钟" },
            { value: "15", label: "每 15 分钟" },
            { value: "30", label: "每 30 分钟" },
            { value: "60", label: "每 60 分钟" },
          ]}
        />
        <small className="muted">保存后生效，自动拉取项目、我负责的执行及指派给我的 BUG；最小化到托盘后仍会同步。</small>
      </label>
      <div className="segmented" role="group" aria-label="连接方式">
        <button
          type="button"
          className={mode === "account" ? "active" : ""}
          aria-pressed={mode === "account"}
          disabled={locked}
          onClick={() => changeMode("account")}
        >
          <LogIn size={14} />
          账号登录
        </button>
        <button
          type="button"
          className={mode === "token" ? "active" : ""}
          aria-pressed={mode === "token"}
          disabled={locked}
          onClick={() => changeMode("token")}
        >
          <KeyRound size={14} />
          访问令牌
        </button>
      </div>
      {mode === "account" ? (
        <>
          <label>
            <span className="zentao-field-title">禅道账号</span>
            <input
              required
              disabled={locked}
              autoComplete="username"
              value={account}
              onChange={(event) => {
                setAccount(event.target.value);
                setPassword("");
                setEditingPassword(true);
              }}
            />
          </label>
          <label>
            <span className="zentao-field-title">
              密码 {helpButton}
              {retainingPassword && (
                <button
                  type="button"
                  className="text-link"
                  disabled={locked}
                  onClick={() => {
                    setEditingPassword(true);
                    setPassword("");
                  }}
                >
                  修改密码
                </button>
              )}
              {hasSavedPassword &&
                editingPassword &&
                account.trim() === connection.loginAccount && (
                  <button
                    type="button"
                    className="text-link"
                    disabled={locked}
                    onClick={() => {
                      setEditingPassword(false);
                      setPassword("");
                    }}
                  >
                    保留原密码
                  </button>
                )}
            </span>
            <input
              required={!saved && !retainingPassword}
              type="password"
              aria-label="禅道密码"
              disabled={locked}
              autoComplete="off"
              readOnly={retainingPassword}
              placeholder={
                preferencesLoading
                  ? "正在检查已保存凭据…"
                  : editingPassword
                    ? "请输入密码"
                    : ""
              }
              value={retainingPassword ? "********" : password}
              onChange={(event) => setPassword(event.target.value)}
            />
          </label>
        </>
      ) : (
        <label>
          <span className="inline-field">访问令牌 {helpButton}</span>
          <input
            required={!existing && !saved}
            type="password"
            aria-label="禅道访问令牌"
            disabled={locked}
            autoComplete="off"
            placeholder={existing ? "留空保留已有令牌" : "请输入访问令牌"}
            value={token}
            onChange={(event) => setToken(event.target.value)}
          />
        </label>
      )}
      {(mode === "account" ||
        (connection.rememberCredentials && !token.trim())) && (
        <div>
          <label className="check-label">
            <input
              type="checkbox"
              disabled={locked}
              checked={rememberCredentials}
              onChange={(event) => setRememberCredentials(event.target.checked)}
            />
            记住登录凭据，令牌失效后自动重新登录
          </label>
          <p className="muted">
            账号密码仅保存在系统钥匙串。平时使用已保存的令牌；认证失效时最多自动登录一次，失败后暂停并提示处理。
            {mode === "token" &&
              " 已保存的登录凭据会保留；取消勾选并保存可清除。"}
          </p>
        </div>
      )}
      {mode === "token" && token.trim() && connection.rememberCredentials && (
        <p className="notice">
          改用手动令牌后，将停用自动登录并清除已保存的登录凭据。
        </p>
      )}
      {needsHttpConsent && (
        <label className="check-label">
          <input
            type="checkbox"
            required={!saved}
            disabled={locked}
            checked={allowInsecureHttp}
            onChange={(event) => setAllowInsecureHttp(event.target.checked)}
          />
          我允许通过未加密的 HTTP 发送账号和密码
          {rememberCredentials && "（包括令牌失效后的自动登录）"}
        </label>
      )}
      {help && (
        <div className="notice">
          账号登录通过禅道官方账号接口获取令牌，并非网页单点登录（SSO）。 公司
          SSO 账号若没有禅道密码，请使用访问令牌。
          令牌保存在系统钥匙串。未勾选“记住登录凭据”时，密码仅用于本次登录；勾选后可在认证失效时自动重新登录。
        </div>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <footer>
        <button
          className="primary"
          disabled={
            busy ||
            preferencesLoading ||
            (!saved && needsHttpConsent && !allowInsecureHttp)
          }
        >
          {saved ? (
            <RefreshCw size={15} />
          ) : loggingIn ? (
            <LogIn size={15} />
          ) : (
            <Save size={15} />
          )}
          {busy
            ? saved
              ? "刷新中…"
              : loggingIn
                ? "登录中…"
                : "保存中…"
            : saved
              ? "重试刷新"
              : loggingIn
                ? "登录并保存"
                : "保存连接"}
        </button>
      </footer>
    </form>
  );
  return inline ? (
    <section className="zentao-inline-form">{content}</section>
  ) : (
    <Modal title="禅道连接" onClose={close}>
      {content}
    </Modal>
  );
}
