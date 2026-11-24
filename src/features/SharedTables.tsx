import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { DateTime } from "luxon";
import { ExternalLink, Pencil, Plus, Table2, Trash2 } from "lucide-react";
import DateRangePicker, { type DateRange } from "../components/DateRangePicker";
import GlassSelect from "../components/GlassSelect";
import Modal from "../components/Modal";
import { api, native } from "../lib/api";
import type { SharedTable, SharedTableInput } from "../lib/types";
import SharedTableGroupManager from "./SharedTableGroupManager";
import "./shared-tables.css";

interface SharedTablesProps {
  workspaceId: string;
  dataRoot: string;
  notify: (message: string) => void;
}

type Dialog =
  | { kind: "edit"; draft: SharedTableInput }
  | { kind: "delete"; table: SharedTable };

const editFields = [
  { key: "name", label: "名称" },
  { key: "creator", label: "创建人" },
  { key: "url", label: "链接地址" },
] as const;

const ungroupedFilter = "__ungrouped__";

const dateFormatter = new Intl.DateTimeFormat("zh-CN", {
  dateStyle: "short",
  timeStyle: "medium",
  hour12: false,
});

function webUrl(value: string) {
  const authority = /^https?:\/\/([^/?#]+)/i.exec(value)?.[1];
  if (!authority || authority.includes("@") || /[\\\p{White_Space}\p{Cc}]/u.test(value)) {
    return undefined;
  }
  try {
    const url = new URL(value);
    return url.hostname ? url.href : undefined;
  } catch {
    return undefined;
  }
}

function TableDate({ value }: { value: string }) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return <>—</>;
  return <time dateTime={value}>{dateFormatter.format(date)}</time>;
}

export default function SharedTables({ workspaceId, dataRoot, notify }: SharedTablesProps) {
  const client = useQueryClient();
  const queryKey = ["shared-tables", workspaceId, dataRoot] as const;
  const groupQueryKey = ["shared-table-groups", workspaceId, dataRoot] as const;
  const { data: tables = [], isPending, isFetching, error: loadError, refetch } = useQuery({
    queryKey,
    queryFn: () => api.listSharedTables(workspaceId),
    enabled: Boolean(workspaceId),
  });
  const groupQuery = useQuery({
    queryKey: groupQueryKey,
    queryFn: () => api.listSharedTableGroups(workspaceId),
    enabled: Boolean(workspaceId),
  });
  const groups = groupQuery.data ?? [];
  const groupNames = new Map(groups.map((group) => [group.id, group.name]));
  const groupOptions = groups.map((group) => ({ value: group.id, label: group.name }));
  const [nameFilter, setNameFilter] = useState("");
  const [groupFilter, setGroupFilter] = useState("");
  const [creatorFilter, setCreatorFilter] = useState("");
  const [createdDateRange, setCreatedDateRange] = useState<DateRange | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [formError, setFormError] = useState("");
  const [managingGroups, setManagingGroups] = useState(false);
  const manageButton = useRef<HTMLButtonElement>(null);
  const restoreManageFocus = useRef(false);
  const filterGroupId = useId();
  const editGroupId = useId();
  const dateRangeId = useId();
  const submitting = useRef(false);
  const { mutate, isPending: busy } = useMutation({
    mutationFn: async (action: Dialog) => {
      if (action.kind === "edit") {
        await api.saveSharedTable(workspaceId, action.draft);
      } else {
        await api.deleteSharedTable(workspaceId, action.table.id, action.table.revision);
      }
    },
    onSuccess: () => client.invalidateQueries({ queryKey, exact: true }),
  });

  useEffect(() => {
    if (!managingGroups && restoreManageFocus.current) {
      restoreManageFocus.current = false;
      manageButton.current?.focus();
    }
  }, [managingGroups]);

  function openDialog(value: Dialog | null) {
    if (submitting.current) return;
    setFormError("");
    setDialog(value);
  }

  function updateDraft(fields: Partial<Pick<SharedTableInput, "name" | "creator" | "url" | "groupId">>) {
    setDialog((current) => current?.kind === "edit"
      ? { kind: "edit", draft: { ...current.draft, ...fields } }
      : current);
  }

  function groupDeleted(id: string) {
    setGroupFilter((current) => current === id ? "" : current);
    // 仅有未保存草稿引用时后端允许删除，保留该草稿的其余内容。
    setDialog((current) => current?.kind === "edit" && current.draft.groupId === id
      ? { kind: "edit", draft: { ...current.draft, groupId: null } }
      : current);
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!native || !workspaceId || !dialog || submitting.current) return;
    let action = dialog;
    if (action.kind === "edit") {
      const { draft } = action;
      const name = draft.name.trim();
      const creator = draft.creator.trim();
      const url = draft.url.trim();
      if (!name || !creator || !url) {
        setFormError("请填写名称、创建人和链接地址。");
        return;
      }
      if (!webUrl(url)) {
        setFormError("请输入有效的 HTTP 或 HTTPS 链接地址。");
        return;
      }
      action = { kind: "edit", draft: { ...draft, name, creator, url } };
    }

    submitting.current = true;
    setFormError("");
    mutate(action, {
      onSuccess: () => {
        setDialog(null);
        notify(action.kind === "edit" ? "共享链接已保存" : "共享链接已删除");
      },
      onError: (error) => setFormError(error.message),
      onSettled: () => { submitting.current = false; },
    });
  }

  const nameTerm = nameFilter.trim().toLocaleLowerCase();
  const creatorTerm = creatorFilter.trim().toLocaleLowerCase();
  const visible = tables.filter((table) => {
    if (!table.name.toLocaleLowerCase().includes(nameTerm) ||
        !table.creator.toLocaleLowerCase().includes(creatorTerm)) return false;
    if (groupFilter && table.groupId !== (groupFilter === ungroupedFilter ? null : groupFilter)) return false;
    if (!createdDateRange) return true;
    // 先把 RFC3339 时间转换为本地日历日，按日期包含起止两天。
    const date = DateTime.fromISO(table.createdAt, { zone: "local" }).toISODate();
    return Boolean(date && date >= createdDateRange.start && date <= createdDateRange.end);
  });
  const hasFilters = Boolean(nameTerm || creatorTerm || groupFilter || createdDateRange);
  const writesDisabled = !native || !workspaceId || busy;

  return (
    <section className="shared-tables">
      {!native && (
        <p className="notice shared-tables-notice" role="status">
          浏览器预览不保存数据。请在栖点桌面应用中管理共享链接。
        </p>
      )}
      <div className="toolbar shared-tables-filters" role="group" aria-label="筛选共享链接">
        <label className="shared-tables-filter-field">
          名称
          <input type="search" value={nameFilter} onChange={(event) => setNameFilter(event.target.value)} />
        </label>
        <div className="shared-tables-filter-field">
          <label htmlFor={filterGroupId}>分组</label>
          <GlassSelect
            id={filterGroupId}
            value={groupFilter}
            onValueChange={setGroupFilter}
            options={[
              { value: "", label: "全部分组" },
              { value: ungroupedFilter, label: "未分组" },
              ...groupOptions,
            ]}
          />
        </div>
        <label className="shared-tables-filter-field">
          创建人
          <input type="search" value={creatorFilter} onChange={(event) => setCreatorFilter(event.target.value)} />
        </label>
        <div className="shared-tables-filter-field shared-tables-date-filter">
          <label htmlFor={dateRangeId}>创建日期</label>
          <DateRangePicker id={dateRangeId} value={createdDateRange} onChange={setCreatedDateRange} />
        </div>
        <button
          type="button"
          className="primary shared-tables-add"
          disabled={writesDisabled}
          onClick={() => openDialog({
            kind: "edit",
            draft: { name: "", creator: "", groupId: null, url: "" },
          })}
        >
          <Plus size={16} /> 新增连接
        </button>
      </div>
      {groupQuery.error && (
        <div className="actions shared-tables-group-error">
          <p className="error" role="alert">读取分组失败。{groupQuery.error.message}</p>
          <button type="button" disabled={groupQuery.isFetching} onClick={() => void groupQuery.refetch()}>
            重试读取分组
          </button>
        </div>
      )}
      {isPending && <p className="muted" role="status">正在加载共享链接…</p>}
      {loadError && (
        <div className="actions">
          <p className="error" role="alert">读取共享链接失败。{loadError.message}</p>
          <button type="button" disabled={isFetching} onClick={() => void refetch()}>
            重试
          </button>
        </div>
      )}
      {!isPending && !loadError && visible.length === 0 && (
        <div className="empty" role="status">
          <Table2 aria-hidden="true" />
          <p>{hasFilters ? "未找到匹配的共享链接" : "暂无共享链接"}</p>
        </div>
      )}
      {visible.length > 0 && (
        <div className="table-wrap shared-tables-scroll" role="region" aria-label="共享链接列表" tabIndex={0}>
          <table className="shared-tables-list">
            <thead>
              <tr>
                <th scope="col">名称</th>
                <th scope="col">分组</th>
                <th scope="col">链接地址</th>
                <th scope="col">创建人</th>
                <th scope="col">创建时间</th>
                <th scope="col">更新时间</th>
                <th scope="col">操作</th>
              </tr>
            </thead>
            <tbody>
              {visible.map((table) => {
                const href = webUrl(table.url);
                return (
                  <tr key={table.id}>
                    <th scope="row">{table.name}</th>
                    <td>{table.groupId === null ? "未分组" : groupNames.get(table.groupId) ?? "—"}</td>
                    <td>
                      {href ? (
                        <a
                          className="shared-tables-link"
                          href={href}
                          title={table.url}
                          target="_blank"
                          rel="noopener noreferrer"
                          onClick={native ? (event) => {
                            event.preventDefault();
                            void openUrl(href).catch(() => notify("无法在浏览器中打开共享链接"));
                          } : undefined}
                        >
                          <span>{table.url}</span>
                          <ExternalLink size={13} aria-hidden="true" />
                        </a>
                      ) : table.url}
                    </td>
                    <td>{table.creator}</td>
                    <td><TableDate value={table.createdAt} /></td>
                    <td><TableDate value={table.updatedAt} /></td>
                    <td>
                      <div className="shared-tables-row-actions">
                        <button
                          type="button"
                          className="icon-button"
                          aria-label={`更新 ${table.name}`}
                          title={`更新 ${table.name}`}
                          disabled={writesDisabled}
                          onClick={() => openDialog({
                            kind: "edit",
                            draft: {
                              id: table.id,
                              revision: table.revision,
                              name: table.name,
                              creator: table.creator,
                              groupId: table.groupId,
                              url: table.url,
                            },
                          })}
                        >
                          <Pencil size={15} aria-hidden="true" />
                        </button>
                        <button
                          type="button"
                          className="icon-button danger"
                          aria-label={`删除 ${table.name}`}
                          title={`删除 ${table.name}`}
                          disabled={writesDisabled}
                          onClick={() => openDialog({ kind: "delete", table })}
                        >
                          <Trash2 className="danger" size={15} aria-hidden="true" />
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {dialog && (
        <Modal
          title={dialog.kind === "delete" ? "删除共享链接" : dialog.draft.id ? "更新共享链接" : "新增共享链接"}
          onClose={() => openDialog(null)}
          busy={busy}
        >
          <form className="form-grid" noValidate onSubmit={submit}>
            {dialog.kind === "delete" ? (
              <p className="shared-tables-confirm">确定删除「{dialog.table.name}」的记录吗？</p>
            ) : (
              <>
                {editFields.map(({ key, label }) => (
                  <label key={key}>
                    {label}
                    <input
                      type={key === "url" ? "url" : "text"}
                      required
                      disabled={busy}
                      value={dialog.draft[key]}
                      onChange={(event) => updateDraft({ [key]: event.target.value })}
                    />
                  </label>
                ))}
                <div className="shared-tables-group-field">
                  <label htmlFor={editGroupId}>分组（选填）</label>
                  <div className="shared-tables-group-control">
                    <GlassSelect
                      id={editGroupId}
                      value={dialog.draft.groupId ?? ""}
                      onValueChange={(value) => updateDraft({ groupId: value || null })}
                      options={[{ value: "", label: "未分组" }, ...groupOptions]}
                      disabled={busy || !groupQuery.data}
                      placeholder={dialog.draft.groupId ? "当前分组不可用" : "未分组"}
                    />
                    <button
                      type="button"
                      ref={manageButton}
                      aria-haspopup="dialog"
                      aria-label="管理分组"
                      disabled={busy || !groupQuery.data}
                      onClick={() => setManagingGroups(true)}
                    >
                      管理
                    </button>
                  </div>
                </div>
              </>
            )}
            {formError && <p className="error" role="alert">{formError}</p>}
            <footer>
              <button type="button" disabled={busy} onClick={() => openDialog(null)}>取消</button>
              {dialog.kind === "delete" ? (
                <button type="submit" className="primary" disabled={writesDisabled}>
                  {busy ? "删除中…" : "确认删除"}
                </button>
              ) : (
                <button type="submit" className="primary" disabled={writesDisabled}>
                  {busy ? "保存中…" : "保存"}
                </button>
              )}
            </footer>
          </form>
        </Modal>
      )}
      {managingGroups && (
        <SharedTableGroupManager
          workspaceId={workspaceId}
          queryKey={groupQueryKey}
          groups={groups}
          notify={notify}
          onDelete={groupDeleted}
          onClose={() => {
            restoreManageFocus.current = true;
            setManagingGroups(false);
          }}
        />
      )}
    </section>
  );
}
