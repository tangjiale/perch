import { useId, useLayoutEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AppWindow, Copy, Eye, EyeOff, FileUp, KeyRound, Pencil, Plus, Server, Trash2 } from "lucide-react";
import GlassSelect from "../components/GlassSelect";
import Modal from "../components/Modal";
import { api, native } from "../lib/api";
import type {
  PasswordVaultCopyField,
  PasswordVaultDraft,
  PasswordVaultEntry,
  PasswordVaultFields,
  PasswordVaultImportResult,
  PasswordVaultKind,
  PasswordVaultPreview,
} from "../lib/types";
import "./shared-tables.css";
import { ExcelImportDialog } from "./AppExcelImport";
import "./password-vault.css";

interface Props {
  workspaceId: string;
  dataRoot: string;
  notify: (message: string) => void;
}

const tabs = [
  { kind: "application", label: "应用列表", icon: AppWindow },
  { kind: "server", label: "服务器", icon: Server },
] as const;

const labels = {
  appName: "应用名称",
  appCode: "应用编号",
  address: "地址",
  account1: "账号1",
  account2: "账号2",
  ip: "IP",
  account: "账号",
  environment: "环境",
  notes: "备注",
  password: "密码",
};

const columns = {
  application: ["appName", "appCode", "address", "account1", "account2", "password", "notes"],
  server: ["ip", "account", "password", "environment", "notes"],
} as const;

const filterFields = {
  application: ["appName", "appCode", "address", "account", "notes"],
  server: ["ip", "account", "environment", "notes"],
} as const;

type Field = Exclude<keyof typeof labels, "password">;
type FilterField = (typeof filterFields)[PasswordVaultKind][number];
type Editor = { type: "edit"; draft: PasswordVaultDraft; passwordStatus: "unread" | "loading" | "stored" | "draft" };
type Dialog = Editor | { type: "delete"; entry: PasswordVaultEntry } | { type: "import" };

const mask = "********";
const environments = [
  { value: "dev", label: "dev" },
  { value: "test", label: "test" },
  { value: "prod", label: "prod" },
] as const;
const environmentOptions = [...environments];
const environmentFilterOptions = [{ value: "", label: "全部环境" }, ...environments];

function fieldValue(record: PasswordVaultFields, field: Field) {
  const fields: Partial<Record<Field, string>> = record;
  return fields[field] ?? "";
}

function matchesFilter(record: PasswordVaultFields, field: FilterField, term: string) {
  if (field === "account" && record.kind === "application") {
    return [record.account1, record.account2].some((account) => account.toLocaleLowerCase().includes(term));
  }
  return fieldValue(record, field).toLocaleLowerCase().includes(term);
}

function entryName(record: PasswordVaultFields) {
  return record.kind === "application" ? record.appName : record.ip;
}

function newDraft(kind: PasswordVaultKind): PasswordVaultDraft {
  return kind === "application"
    ? { kind, appName: "", appCode: "", address: "", account1: "", account2: "", notes: "", password: "" }
    : { kind, ip: "", account: "", environment: "dev", notes: "", password: "" };
}

export default function PasswordVault(props: Props) {
  return <VaultTabs key={JSON.stringify([props.workspaceId, props.dataRoot])} {...props} />;
}

function VaultTabs(props: Props) {
  const [kind, setKind] = useState<PasswordVaultKind>("application");
  const [actionsTarget, setActionsTarget] = useState<HTMLDivElement | null>(null);
  const tabId = useId();
  const panelId = `${tabId}-panel`;

  return (
    <section className="shared-tables password-vault">
      <header className="password-vault-header">
        <div className="tabs password-vault-tabs" role="tablist" aria-label="密码库分类" onKeyDown={(event) => {
          const other = kind === "application" ? "server" : "application";
          const next = { ArrowLeft: other, ArrowRight: other, Home: "application", End: "server" } as const;
          const destination = next[event.key as keyof typeof next];
          if (!destination) return;
          event.preventDefault();
          setKind(destination);
          document.getElementById(`${tabId}-${destination}`)?.focus();
        }}>
          {tabs.map(({ kind: tab, label, icon: Icon }) => (
            <button
              key={tab}
              id={`${tabId}-${tab}`}
              className={tab === kind ? "active" : undefined}
              type="button"
              role="tab"
              aria-controls={panelId}
              aria-selected={tab === kind}
              tabIndex={tab === kind ? 0 : -1}
              onClick={() => setKind(tab)}
            >
              <Icon size={16} aria-hidden="true" /> {label}
            </button>
          ))}
        </div>
        <div ref={setActionsTarget} className="actions password-vault-actions" />
      </header>
      <div id={panelId} role="tabpanel" aria-labelledby={`${tabId}-${kind}`}>
        <VaultEntries key={kind} kind={kind} actionsTarget={actionsTarget} {...props} />
      </div>
    </section>
  );
}

function VaultEntries({ workspaceId, dataRoot, kind, notify, actionsTarget }: Props & {
  kind: PasswordVaultKind;
  actionsTarget: HTMLDivElement | null;
}) {
  const client = useQueryClient();
  const queryKey = useMemo(() => ["password-vault", workspaceId, dataRoot, kind] as const, [workspaceId, dataRoot, kind]);
  const enabled = native && Boolean(workspaceId);
  const query = useQuery({ queryKey, queryFn: () => api.listPasswordVault(workspaceId, kind), enabled, refetchOnWindowFocus: false });
  const [filters, setFilters] = useState<Partial<Record<FilterField, string>>>({});
  const [revealed, setRevealed] = useState<Map<string, string>>(() => new Map());
  const [viewing, setViewing] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [formError, setFormError] = useState("");
  const [maskRevision, setMaskRevision] = useState(0);
  const [saving, setSaving] = useState(false);
  const secretGeneration = useRef(0);
  const active = useRef(false);
  const writing = useRef(false);
  const filterId = useId();

  useLayoutEffect(() => {
    active.current = true;
    const cache = client.getQueryCache();
    const currentQuery = cache.find({ queryKey, exact: true });

    const unsubscribe = cache.subscribe((event) => {
      if (event.query === currentQuery && event.type === "updated" &&
          (event.action.type === "invalidate" || event.action.type === "fetch" || event.action.type === "success")) {
        conceal(true);
      }
    });
    const onBlur = () => conceal(false);
    const onVisibilityChange = () => {
      if (document.hidden) onBlur();
    };
    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      active.current = false;
      secretGeneration.current += 1;
      unsubscribe();
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [client, queryKey]);

  const fields = filterFields[kind];
  const terms = fields.map((field) => [field, filters[field]?.trim().toLocaleLowerCase()] as const);
  const hasFilters = terms.some(([, term]) => Boolean(term));
  const entries = (query.data ?? []).filter((entry) => terms.every(([field, term]) =>
    !term || matchesFilter(entry, field, term)
  ));
  const withPassword = entries.filter((entry) => entry.hasPassword);
  const allVisible = withPassword.length > 0 && withPassword.every((entry) => revealed.has(entry.id));
  const canRead = enabled && !query.isFetching && !query.isError;
  const canWrite = enabled && !saving;

  function clearRevealed() {
    secretGeneration.current += 1;
    setRevealed((current) => current.size ? new Map() : current);
    setViewing(null);
  }

  function conceal(discardStored: boolean) {
    clearRevealed();
    setMaskRevision((current) => current + 1);
    // 刷新时清除已读取的密码。保留用户输入，并要求显式读取旧密码后再提交。
    setDialog((current) => current?.type === "edit" &&
        (current.passwordStatus === "loading" || (discardStored && current.passwordStatus === "stored"))
      ? { ...current, passwordStatus: "unread", draft: { ...current.draft, password: "" } }
      : current);
  }

  function changeFilter(field: FilterField, value: string) {
    conceal(true);
    setFilters((current) => ({ ...current, [field]: value }));
  }

  function openDialog(next: Dialog | null) {
    setFormError("");
    setDialog(next);
  }

  function closeDialog() {
    if (writing.current) return;
    clearRevealed();
    openDialog(null);
  }

  function readPasswords(records: Pick<PasswordVaultEntry, "id" | "revision">[]) {
    return api.revealPasswordVault(workspaceId, kind, records.map(({ id, revision }) => ({ id, revision })));
  }

  async function copyField(entry: PasswordVaultEntry, field: PasswordVaultCopyField) {
    try {
      await api.copyPasswordVault(workspaceId, kind, { id: entry.id, revision: entry.revision }, field);
      if (active.current) notify(`已复制${labels[field]}。`);
    } catch (cause) {
      if (active.current) notify(cause instanceof Error ? cause.message : "复制失败，请重试。");
    }
  }

  function copyButton(entry: PasswordVaultEntry, field: PasswordVaultCopyField) {
    return (
      <button type="button" className="icon-button" title={`复制${labels[field]}`}
        aria-label={`复制 ${entryName(entry)} 的${labels[field]}`} disabled={!canRead}
        onClick={() => void copyField(entry, field)}>
        <Copy size={15} aria-hidden="true" />
      </button>
    );
  }

  async function togglePasswords(records: PasswordVaultEntry[], action: string) {
    if (!canRead || !records.length) return;
    const generation = ++secretGeneration.current;
    setViewing(null);
    if (viewing === action) return;
    if (records.every(({ id }) => revealed.has(id))) {
      setRevealed((current) => {
        const next = new Map(current);
        for (const { id } of records) next.delete(id);
        return next;
      });
      return;
    }

    setViewing(action);
    try {
      const passwords = await readPasswords(records);
      if (generation !== secretGeneration.current) return;
      const byId = new Map(passwords.map(({ id, password }) => [id, password]));
      const requested = records.map(({ id }) => {
        const password = byId.get(id);
        if (password === undefined) throw new Error();
        return [id, password] as const;
      });
      setRevealed((current) => new Map([...current, ...requested]));
    } catch {
      if (generation === secretGeneration.current) notify("读取密码失败，请刷新列表后重试。");
    } finally {
      if (generation === secretGeneration.current) setViewing(null);
    }
  }

  async function readEditorPassword(id: string, revision: number) {
    if (!canRead || writing.current) return;
    clearRevealed();
    const generation = secretGeneration.current;
    setFormError("");
    setDialog((current) => current?.type === "edit" && current.draft.id === id
      ? { ...current, passwordStatus: "loading" }
      : current);
    try {
      const [record] = await readPasswords([{ id, revision }]);
      if (generation !== secretGeneration.current) return;
      if (record?.id !== id) throw new Error();
      setDialog((current) => current?.type === "edit" && current.draft.id === id
        ? { ...current, draft: { ...current.draft, password: record.password }, passwordStatus: "stored" }
        : current);
    } catch {
      if (generation === secretGeneration.current) {
        setDialog((current) => current?.type === "edit" && current.draft.id === id
          ? { ...current, passwordStatus: "unread" }
          : current);
        setFormError("读取密码失败，请刷新列表后重试。");
      }
    }
  }

  function editEntry(entry: PasswordVaultEntry) {
    if (!canRead || writing.current) return;
    const { hasPassword, ...fields } = entry;
    openDialog({ type: "edit", draft: { ...fields, password: "" }, passwordStatus: hasPassword ? "loading" : "draft" });
    if (hasPassword) void readEditorPassword(entry.id, entry.revision);
    else clearRevealed();
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!canWrite || !dialog || dialog.type === "import" || writing.current) return;
    if (dialog.type === "edit" && dialog.passwordStatus !== "stored" && dialog.passwordStatus !== "draft") return;
    writing.current = true;
    setSaving(true);
    setFormError("");
    try {
      if (dialog.type === "edit") {
        await api.savePasswordVault(workspaceId, dialog.draft);
      } else {
        await api.deletePasswordVault(workspaceId, kind, dialog.entry.id, dialog.entry.revision);
      }
      if (active.current) {
        clearRevealed();
        openDialog(null);
        notify(dialog.type === "edit" ? "密码记录已保存。" : "密码记录已删除。");
      }
      void client.invalidateQueries({ queryKey, exact: true });
    } catch {
      if (active.current) setFormError(dialog.type === "edit" ? "保存失败，请核对输入或刷新列表后重试。" : "删除失败，请刷新列表后重试。");
    } finally {
      writing.current = false;
      if (active.current) setSaving(false);
    }
  }

  const label = kind === "application" ? "应用" : "服务器";

  return (
    <>
      {actionsTarget && createPortal(
        <>
          <button type="button" className="password-vault-reveal" aria-pressed={allVisible} aria-busy={viewing === "all"} disabled={!canRead || !withPassword.length}
            onClick={() => void togglePasswords(withPassword, "all")}>
            {allVisible ? <EyeOff size={16} aria-hidden="true" /> : <Eye size={16} aria-hidden="true" />}
            {viewing === "all" ? "读取密码中…" : allVisible ? "隐藏全部密码" : "查看全部密码"}
          </button>
          <button type="button" onClick={() => { clearRevealed(); openDialog({ type: "import" }); }}>
            <FileUp size={16} aria-hidden="true" /> 导入 Excel
          </button>
          <button type="button" className="primary" disabled={!canWrite} onClick={() => {
            clearRevealed();
            openDialog({ type: "edit", draft: newDraft(kind), passwordStatus: "draft" });
          }}>
            <Plus size={16} aria-hidden="true" /> 新增密码
          </button>
        </>,
        actionsTarget,
      )}
      <div className="toolbar shared-tables-filters password-vault-filters" role="group" aria-label={`筛选${label}密码记录`}>
        {fields.map((field) => field === "environment" ? (
          <div key={field} className="shared-tables-filter-field">
            <label htmlFor={filterId}>{labels[field]}</label>
            <GlassSelect id={filterId} value={filters[field] ?? ""} options={environmentFilterOptions} onValueChange={(value) => changeFilter(field, value)} />
          </div>
        ) : (
          <label key={field} className="shared-tables-filter-field">
            {labels[field]}
            <input type="search" value={filters[field] ?? ""} onChange={(event) => changeFilter(field, event.currentTarget.value)} />
          </label>
        ))}
      </div>

      {!native && <p className="notice" role="status">请在栖点桌面应用中查看和管理密码记录。</p>}
      {enabled && query.isPending && <p className="muted" role="status">正在加载密码记录…</p>}
      {query.isError && (
        <div className="actions">
          <p className="error" role="alert">读取密码记录失败。</p>
          <button type="button" disabled={query.isFetching} onClick={() => void query.refetch()}>重试</button>
        </div>
      )}
      {enabled && !query.isPending && !query.isError && !entries.length && (
        <div className="empty" role="status">
          <KeyRound aria-hidden="true" />
          <p>{hasFilters ? "未找到匹配的记录" : `暂无${label}密码记录`}</p>
        </div>
      )}
      {!!entries.length && (
        <div className="table-wrap shared-tables-scroll" role="region" aria-label={`${label}密码记录`} tabIndex={0}>
          <table className={`shared-tables-list password-vault-list password-vault-list-${kind}`}>
            <thead>
              <tr>
                {columns[kind].map((field) => <th key={field} scope="col" data-field={field}>{labels[field]}</th>)}
                <th scope="col">操作</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((entry) => (
                <tr key={entry.id}>
                  <th scope="row">{entryName(entry)}</th>
                  {columns[kind].slice(1).map((field) => {
                    if (field === "password") {
                      return (
                        <td key={field}>
                          <div className="password-vault-secret">
                            <span>{canRead && revealed.has(entry.id) ? revealed.get(entry.id) : entry.hasPassword ? mask : "—"}</span>
                            {entry.hasPassword && (
                              <>
                                <button
                                  type="button"
                                  className="icon-button"
                                  aria-label={`${revealed.has(entry.id) ? "隐藏" : "查看"} ${entryName(entry)} 的密码`}
                                  aria-pressed={revealed.has(entry.id)}
                                  aria-busy={viewing === entry.id}
                                  disabled={!canRead}
                                  onClick={() => void togglePasswords([entry], entry.id)}
                                >
                                  {revealed.has(entry.id) ? <EyeOff size={15} aria-hidden="true" /> : <Eye size={15} aria-hidden="true" />}
                                </button>
                                {copyButton(entry, field)}
                              </>
                            )}
                          </div>
                        </td>
                      );
                    }
                    const value = fieldValue(entry, field);
                    return (
                      <td key={field}>
                        {value && (field === "account" || field === "account1" || field === "account2") ? (
                          <div className="password-vault-account"><span>{value}</span>{copyButton(entry, field)}</div>
                        ) : value || "—"}
                      </td>
                    );
                  })}
                  <td>
                    <div className="shared-tables-row-actions">
                      <button type="button" className="icon-button" aria-label={`编辑 ${entryName(entry)}`} title="编辑"
                        disabled={!canRead || !canWrite} onClick={() => void editEntry(entry)}>
                        <Pencil size={15} aria-hidden="true" />
                      </button>
                      <button type="button" className="icon-button danger" aria-label={`删除 ${entryName(entry)}`} title="删除"
                        disabled={!canRead || !canWrite} onClick={() => {
                          clearRevealed();
                          openDialog({ type: "delete", entry });
                        }}>
                        <Trash2 size={15} aria-hidden="true" />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {dialog?.type === "edit" && (
        <Modal title={`${dialog.draft.id ? "编辑" : "新增"}${label}密码记录`} busy={saving} onClose={closeDialog}>
          <VaultEditor
            editor={dialog}
            busy={saving}
            canRead={canRead}
            maskRevision={maskRevision}
            error={formError}
            onSubmit={submit}
            onClose={closeDialog}
            onReadPassword={readEditorPassword}
            onChange={(draft, passwordChanged) => setDialog((current) => current?.type === "edit"
              ? {
                ...current,
                draft: { ...draft, password: passwordChanged ? draft.password : current.draft.password },
                passwordStatus: passwordChanged ? "draft" : current.passwordStatus,
              }
              : current)}
          />
        </Modal>
      )}
      {dialog?.type === "delete" && (
        <Modal title="删除密码记录" busy={saving} onClose={closeDialog}>
          <form onSubmit={submit}>
            <p className="shared-tables-confirm">确定删除「{entryName(dialog.entry)}」的密码记录？</p>
            {formError && <p className="error" role="alert">{formError}</p>}
            <footer className="modal-actions">
              <button type="button" disabled={saving} onClick={closeDialog}>取消</button>
              <button type="submit" className="danger" disabled={saving}>{saving ? "删除中…" : "删除"}</button>
            </footer>
          </form>
        </Modal>
      )}
      {dialog?.type === "import" && (
        <ExcelImportDialog<PasswordVaultPreview["rows"][number], PasswordVaultImportResult>
          title={`导入${label}密码记录`}
          className="password-vault-import"
          previewLabel={`${label}密码记录`}
          previewColumns={columns[kind].map((field) => ({
            label: labels[field],
            render: (record) => field === "password" ? (record.hasPassword ? mask : "—") : fieldValue(record, field) || "—",
          }))}
          template={{
            name: `${label}密码库导入模板.xlsx`,
            file: kind === "application" ? "password-vault-applications.xlsx" : "password-vault-servers.xlsx",
            save: (path) => api.savePasswordVaultTemplate(kind, path),
          }}
          previewSource={(source) => api.previewPasswordVaultExcel(kind, source)}
          importSource={async (source) => {
            const result = await api.importPasswordVaultExcel(workspaceId, kind, source);
            void client.invalidateQueries({ queryKey, exact: true });
            return result;
          }}
          confirmEnabled={Boolean(workspaceId)}
          summarize={({ created, skipped }) => `已导入 ${created} 条${label}密码记录，跳过 ${skipped} 条完全相同的记录。`}
          notify={notify}
          onClose={closeDialog}
          protectErrors
        />
      )}
    </>
  );
}

function VaultEditor({ editor, busy, canRead, maskRevision, error, onChange, onReadPassword, onSubmit, onClose }: {
  editor: Editor;
  busy: boolean;
  error: string;
  canRead: boolean;
  maskRevision: number;
  onChange: (draft: PasswordVaultDraft, passwordChanged?: boolean) => void;
  onReadPassword: (id: string, revision: number) => Promise<void>;
  onSubmit: (event: FormEvent) => void;
  onClose: () => void;
}) {
  const environmentId = useId();
  const passwordId = useId();
  const [shownAt, setShownAt] = useState<number | null>(null);
  const showPassword = shownAt === maskRevision;
  const { draft, passwordStatus } = editor;
  const ready = passwordStatus === "draft" || passwordStatus === "stored";

  return (
    <form className="form-grid password-vault-form" onSubmit={onSubmit} autoComplete="off">
      {columns[draft.kind].map((field) => {
        if (field === "environment" && draft.kind === "server") {
          return (
            <div key={field}>
              <label htmlFor={environmentId}>{labels[field]}</label>
              <GlassSelect id={environmentId} value={draft.environment} disabled={busy} options={environmentOptions}
                onValueChange={(value) => {
                  const environment = environments.find((option) => option.value === value);
                  if (environment) onChange({ ...draft, environment: environment.value });
                }} />
            </div>
          );
        }
        if (field === "password") {
          return (
            <div key={field}>
              <label htmlFor={passwordId}>密码</label>
              <div className="inline-field">
                <input id={passwordId} type={showPassword ? "text" : "password"} autoComplete="new-password"
                  spellCheck={false} autoCapitalize="none" disabled={!ready || busy} value={draft.password}
                  onChange={(event) => onChange({ ...draft, password: event.currentTarget.value }, true)} />
                <button type="button" aria-controls={passwordId} disabled={!ready || busy}
                  aria-label={showPassword ? "隐藏密码" : "查看密码"} aria-pressed={showPassword}
                  onClick={() => setShownAt(showPassword ? null : maskRevision)}>
                  {showPassword ? <EyeOff size={16} aria-hidden="true" /> : <Eye size={16} aria-hidden="true" />}
                </button>
              </div>
              {passwordStatus === "loading" && <span className="muted" role="status">正在读取密码…</span>}
              {passwordStatus === "unread" && (
                <button type="button" className="password-vault-read-again" disabled={!canRead || busy} onClick={() => {
                  if (draft.id && draft.revision !== undefined) void onReadPassword(draft.id, draft.revision);
                }}>重新读取密码</button>
              )}
            </div>
          );
        }
        return (
          <label key={field}>
            {labels[field]}
            {field === "notes"
              ? <textarea rows={3} value={draft.notes} disabled={busy} onChange={(event) => onChange({ ...draft, notes: event.currentTarget.value })} />
              : <input value={fieldValue(draft, field)} disabled={busy} required={field === "appName" || field === "ip"}
                  onChange={(event) => onChange({ ...draft, [field]: event.currentTarget.value })} />}
          </label>
        );
      })}
      {error && <p className="error" role="alert">{error}</p>}
      <footer className="modal-actions">
        <button type="button" disabled={busy} onClick={onClose}>取消</button>
        <button type="submit" className="primary" disabled={!ready || busy}>{busy ? "保存中…" : "保存"}</button>
      </footer>
    </form>
  );
}
