import { useEffect, useId, useMemo, useRef, useState } from "react";
import { ChevronRight, Folder, Link2, Pencil } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import GlassSelect from "../components/GlassSelect";
import Modal from "../components/Modal";
import { api, native } from "../lib/api";
import type { BrowserBookmarkAppInput, BrowserBookmarkImportResult, Category, PageProps } from "../lib/types";
import { defaultBookmarkCategory, indexBookmarks, type BookmarkCategory, type BookmarkEntry, type BookmarkFolder } from "./browser-bookmark-draft";
import "./browser-bookmark-import.css";

type BookmarkDraft = Pick<BrowserBookmarkAppInput, "name" | "category">;
const branchPageSize = 120;

interface Props extends Pick<PageProps, "refresh" | "notify"> {
  workspaceId: string;
  categories: Category[];
  onClose: () => void;
}

function SelectionBox({ name, folder, count, total, disabled, onChange }: {
  name: string;
  folder: boolean;
  count: number;
  total: number;
  disabled: boolean;
  onChange: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const mixed = count > 0 && count < total;
  useEffect(() => {
    if (input.current) input.current.indeterminate = mixed;
  }, [mixed]);
  return (
    <input
      ref={input}
      type="checkbox"
      aria-label={`选择${folder ? "文件夹" : "书签"} ${name}`}
      aria-checked={mixed ? "mixed" : undefined}
      checked={total > 0 && count === total}
      disabled={disabled || total === 0}
      onChange={onChange}
    />
  );
}

function CategoryField({ value, categories, disabled, onChange }: {
  value: BookmarkCategory;
  categories: Category[];
  disabled: boolean;
  onChange: (category: BookmarkCategory) => void;
}) {
  const controlId = useId();
  const selected = value?.kind === "existing" ? `category:${value.id}` : value?.kind === "name" ? "new" : "none";
  return (
    <>
      <div className="bookmark-import-field">
        <label htmlFor={controlId}>所属分类</label>
        <GlassSelect
          id={controlId}
          value={selected}
          options={[
            { value: "none", label: "未分类" },
            ...categories.map((category) => ({ value: `category:${category.id}`, label: category.name })),
            { value: "new", label: "新建分类" },
          ]}
          onValueChange={(option) => {
            if (option === "new") onChange({ kind: "name", name: "" });
            else if (option === "none") onChange(null);
            else onChange({ kind: "existing", id: option.slice("category:".length) });
          }}
          disabled={disabled}
          placeholder="请选择分类"
        />
      </div>
      {value?.kind === "name" && (
        <label className="bookmark-import-field">
          新分类名称
          <input value={value.name} disabled={disabled} onChange={(event) => onChange({ kind: "name", name: event.target.value })} />
        </label>
      )}
    </>
  );
}

function FolderEditor({ folder, initialCategory, categories, disabled, onApply }: {
  folder: BookmarkFolder;
  initialCategory: BookmarkCategory;
  categories: Category[];
  disabled: boolean;
  onApply: (category: BookmarkCategory) => void;
}) {
  const [category, setCategory] = useState(initialCategory);
  const [applied, setApplied] = useState(false);
  const count = folder.end - folder.start;
  return (
    <>
      <CategoryField value={category} categories={categories} disabled={disabled} onChange={(next) => {
        setCategory(next);
        setApplied(false);
      }} />
      <button
        type="button"
        disabled={disabled || count === 0 || (category?.kind === "name" && !category.name.trim())}
        onClick={() => {
          onApply(category);
          setApplied(true);
        }}
      >
        应用到文件夹的 {count} 项书签
      </button>
      {applied && <p className="muted" role="status">已调整 {count} 项书签的分类。</p>}
    </>
  );
}

export default function BrowserBookmarkImport({ workspaceId, categories, refresh, notify, onClose }: Props) {
  const query = useQuery({
    queryKey: ["browser-bookmarks"],
    queryFn: () => api.readBrowserBookmarks(),
    enabled: native,
    retry: false,
    refetchOnMount: "always",
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    gcTime: 0,
  });
  const model = useMemo(() => indexBookmarks(query.data ?? []), [query.data]);
  const categoryIds = useMemo(() => new Map(categories.map(({ id, name }) => [name.trim().toLowerCase(), id])), [categories]);
  const [browserId, setBrowserId] = useState("");
  const [profileIds, setProfileIds] = useState(new Map<string, string>());
  const [selected, setSelected] = useState(new Set<string>());
  const [expanded, setExpanded] = useState(new Map<string, boolean>());
  const [branchLimits, setBranchLimits] = useState(new Map<string, number>());
  const [drafts, setDrafts] = useState(new Map<string, Partial<BookmarkDraft>>());
  const [folderCategories, setFolderCategories] = useState(new Map<string, BookmarkCategory>());
  const [inspected, setInspected] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const submitting = useRef(false);
  const tabId = useId();
  const browser = model.browsers.find((source) => source.id === browserId)
    ?? model.browsers.find((source) => source.profiles.some((profile) => profile.nodes.length > 0))
    ?? model.browsers.find((source) => source.status === "ready")
    ?? model.browsers[0];
  const profile = browser?.profiles.find((item) => item.id === profileIds.get(browser.id))
    ?? browser?.profiles.find((item) => item.nodes.length > 0)
    ?? browser?.profiles[0];
  const profileKey = JSON.stringify([browser?.id, profile?.id]);
  const inspectedEntry = model.entries.get(inspected);
  const locked = busy || query.isFetching;

  const { chosen, totals } = useMemo(() => {
    const chosen = [];
    const totals = [0];
    for (const page of model.pages) {
      if (selected.has(page.key)) chosen.push(page);
      totals.push(chosen.length);
    }
    return { chosen, totals };
  }, [model.pages, selected]);

  function selectBrowser(id: string) {
    setBrowserId(id);
    setInspected("");
  }

  function selectProfile(id: string, nextProfile: string) {
    setProfileIds((previous) => new Map(previous).set(id, nextProfile));
    setInspected("");
  }

  function checkEntry(entry: BookmarkEntry) {
    setSelected((previous) => {
      const next = new Set(previous);
      const pages = model.pages.slice(entry.start, entry.end);
      const remove = pages.every((page) => previous.has(page.key));
      for (const page of pages) {
        if (remove) next.delete(page.key);
        else next.add(page.key);
      }
      return next;
    });
  }

  function updateDraft(key: string, fields: Partial<BookmarkDraft>) {
    setDrafts((previous) => new Map(previous).set(key, { ...previous.get(key), ...fields }));
  }

  function applyFolder(folder: BookmarkFolder, category: BookmarkCategory) {
    setFolderCategories((previous) => new Map(previous).set(folder.key, category));
    setDrafts((previous) => {
      const next = new Map(previous);
      for (const page of model.pages.slice(folder.start, folder.end)) {
        next.set(page.key, { ...next.get(page.key), category });
      }
      return next;
    });
  }

  function draftFor(page: Extract<BookmarkEntry, { kind: "bookmark" }>): BookmarkDraft {
    return { name: page.name, category: defaultBookmarkCategory(page.group, categoryIds), ...drafts.get(page.key) };
  }

  function folderCategory(folder: BookmarkFolder): BookmarkCategory {
    const applied = folderCategories.get(folder.key);
    return applied === undefined ? defaultBookmarkCategory(folder.name, categoryIds) : applied;
  }

  function close() {
    if (!submitting.current) onClose();
  }

  async function submit() {
    if (!native || !workspaceId || locked || submitting.current || !chosen.length) return;
    setError("");
    const items: BrowserBookmarkAppInput[] = [];
    for (const page of chosen) {
      const draft = draftFor(page);
      const name = draft.name.trim();
      const category = draft.category?.kind === "name" ? { ...draft.category, name: draft.category.name.trim() } : draft.category;
      const reason = !name ? "请填写应用名称"
        : category?.kind === "name" && !category.name ? "请填写新分类名称"
        : category?.kind === "existing" && !categories.some((item) => item.id === category.id) ? "请重新选择分类" : "";
      if (reason) {
        selectBrowser(page.browserId);
        selectProfile(page.browserId, page.profileId);
        setInspected(page.key);
        setError(`「${page.name}」${reason}。`);
        return;
      }
      items.push({ name, category, url: page.url });
    }
    submitting.current = true;
    setBusy(true);
    let result: BrowserBookmarkImportResult;
    try {
      result = await api.importBrowserBookmarkApps(workspaceId, items);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      submitting.current = false;
      setBusy(false);
      return;
    }

    const summary = `已导入 ${result.created} 项，重复 URL 跳过 ${result.skipped} 项，新建 ${result.categoriesCreated} 个分类。`;
    let refreshFailed = false;
    try {
      const refreshed = await refresh();
      refreshFailed = Boolean(refreshed && typeof refreshed === "object" && "isError" in refreshed && refreshed.isError === true);
    } catch {
      refreshFailed = true;
    }
    notify(refreshFailed ? `${summary} 应用列表刷新失败。` : summary);
    onClose();
  }

  function renderTree(entries: BookmarkEntry[], branch: string, depth = 0) {
    const limit = branchLimits.get(branch) ?? branchPageSize;
    return (
      <ul className="bookmark-import-tree">
        {entries.slice(0, limit).map((entry) => {
          const folder = entry.kind === "folder";
          const canExpand = folder && entry.children.length > 0;
          const open = canExpand && (expanded.get(entry.key) ?? depth === 0);
          const editable = entry.end > entry.start;
          const title = `${folder ? "调整文件夹分类" : "编辑书签"} ${entry.name}`;
          const Icon = folder ? Folder : Link2;
          return (
            <li key={entry.key}>
              <div className="bookmark-import-node" style={{ paddingInlineStart: `${Math.min(depth, 8) * 14 + 4}px` }}>
                {canExpand ? (
                  <button
                    type="button"
                    className="icon-button bookmark-import-disclosure"
                    aria-label={`${open ? "折叠" : "展开"} ${entry.name}`}
                    aria-expanded={open}
                    disabled={locked}
                    onClick={() => setExpanded((previous) => new Map(previous).set(entry.key, !open))}
                  >
                    <ChevronRight size={15} aria-hidden="true" />
                  </button>
                ) : <span className="bookmark-import-disclosure" />}
                <SelectionBox
                  name={entry.name}
                  folder={folder}
                  count={totals[entry.end] - totals[entry.start]}
                  total={entry.end - entry.start}
                  disabled={locked}
                  onChange={() => checkEntry(entry)}
                />
                <button
                  type="button"
                  className="bookmark-import-node-editor"
                  aria-label={title}
                  aria-pressed={inspected === entry.key}
                  title={editable ? title : folder ? "此文件夹没有可导入的网页" : "仅导入不含账号密码的 HTTP 或 HTTPS 地址"}
                  disabled={locked || !editable}
                  onClick={() => setInspected(entry.key)}
                >
                  <Icon size={15} aria-hidden="true" />
                  <span>{drafts.get(entry.key)?.name || entry.name}</span>
                  {editable && <Pencil size={13} aria-hidden="true" className="bookmark-import-edit-icon" />}
                </button>
              </div>
              {folder && open && renderTree(entry.children, entry.key, depth + 1)}
            </li>
          );
        })}
        {limit < entries.length && (
          <li>
            <button type="button" className="bookmark-import-more text-link" disabled={locked}
              onClick={() => setBranchLimits((previous) => new Map(previous).set(branch, limit + branchPageSize))}>
              显示更多（剩余 {entries.length - limit} 项）
            </button>
          </li>
        )}
      </ul>
    );
  }

  const activeBrowserIndex = model.browsers.findIndex((source) => source.id === browser?.id);
  const currentDraft = inspectedEntry?.kind === "bookmark" ? draftFor(inspectedEntry) : null;

  return (
    <Modal title="从浏览器导入书签" className="bookmark-import-dialog" wide busy={busy} onClose={close}>
      {!native && <p className="notice" role="status">请在栖点桌面应用读取并导入本机浏览器书签。</p>}
      {native && query.isPending && <p className="bookmark-import-status muted" role="status">正在读取浏览器书签…</p>}
      {query.error && (
        <div className="actions bookmark-import-status">
          <p className="error" role="alert">读取书签失败。{query.error.message}</p>
          <button type="button" disabled={locked} onClick={() => void query.refetch()}>重新读取</button>
        </div>
      )}
      {query.data && !browser && <p className="bookmark-import-status muted" role="status">暂无可读取的浏览器书签。</p>}
      {browser && (
        <>
          <div className="bookmark-import-tabs" role="tablist" aria-label="浏览器" onKeyDown={(event) => {
            if (locked || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
            event.preventDefault();
            const length = model.browsers.length;
            const index = event.key === "Home" ? 0 : event.key === "End" ? length - 1
              : (activeBrowserIndex + (event.key === "ArrowRight" ? 1 : -1) + length) % length;
            selectBrowser(model.browsers[index].id);
            document.getElementById(`${tabId}-${index}`)?.focus();
          }}>
            {model.browsers.map((source, index) => (
              <button key={source.id} type="button" role="tab" id={`${tabId}-${index}`}
                aria-selected={source.id === browser.id} aria-controls={`${tabId}-panel`}
                tabIndex={source.id === browser.id ? 0 : -1} disabled={locked}
                onClick={() => selectBrowser(source.id)}>{source.name}</button>
            ))}
          </div>
          <div id={`${tabId}-panel`} role="tabpanel" aria-labelledby={`${tabId}-${activeBrowserIndex}`}>
            {browser.status === "ready" && browser.message && <p className="bookmark-import-read-note muted" role="status">{browser.message}</p>}
            {browser.status !== "ready" ? (
              <div className="bookmark-import-status">
                <p className={browser.status === "error" ? "error" : "muted"} role={browser.status === "error" ? "alert" : "status"}>
                  {browser.message ?? (browser.status === "error" ? "无法读取此浏览器的书签。" : "此浏览器暂无可读取的用户资料。")}
                </p>
                <button type="button" disabled={locked} onClick={() => void query.refetch()}>重新读取</button>
              </div>
            ) : profile ? (
              <>
                <div className="bookmark-import-profile">
                  <label htmlFor={`${tabId}-profile`}>用户资料</label>
                  <GlassSelect id={`${tabId}-profile`} value={profile.id} disabled={locked}
                    options={browser.profiles.map(({ id, name }) => ({ value: id, label: name }))}
                    onValueChange={(id) => selectProfile(browser.id, id)} />
                  <span className="muted">{profile.end - profile.start} 项可导入</span>
                </div>
                {profile.nodes.length ? (
                  <div className="bookmark-import-grid">
                    <div className="bookmark-import-tree-pane" role="region" aria-label="书签树" tabIndex={0} key={profileKey}>
                      {renderTree(profile.nodes, profileKey)}
                    </div>
                    <aside className="bookmark-import-editor" aria-label="编辑导入内容">
                      {inspectedEntry && (
                        <h3 title={inspectedEntry.name}>{inspectedEntry.name}</h3>
                      )}
                      {inspectedEntry?.kind === "bookmark" && currentDraft ? (
                        <>
                          <p className="muted bookmark-import-url" title={inspectedEntry.url}>{inspectedEntry.url}</p>
                          <label className="bookmark-import-field">
                            应用名称
                            <input value={currentDraft.name} disabled={locked}
                              onChange={(event) => updateDraft(inspectedEntry.key, { name: event.target.value })} />
                          </label>
                          <CategoryField value={currentDraft.category} categories={categories} disabled={locked}
                            onChange={(category) => updateDraft(inspectedEntry.key, { category })} />
                        </>
                      ) : inspectedEntry?.kind === "folder" ? (
                        <FolderEditor key={inspectedEntry.key} folder={inspectedEntry} categories={categories} disabled={locked}
                          initialCategory={folderCategory(inspectedEntry)}
                          onApply={(category) => applyFolder(inspectedEntry, category)} />
                      ) : <p className="muted">点击书签可编辑名称和分类，点击文件夹可批量调整分类。</p>}
                    </aside>
                  </div>
                ) : <p className="bookmark-import-status muted" role="status">此用户资料没有书签。</p>}
              </>
            ) : <p className="bookmark-import-status muted" role="status">此浏览器没有书签。</p>}
          </div>
        </>
      )}
      {query.isFetching && !query.isPending && <p className="muted" role="status">正在重新读取书签…</p>}
      {error && <p className="bookmark-import-error error" role="alert">{error}</p>}
      <footer className="bookmark-import-footer">
        {native && <p className="muted" role="status">已选 {chosen.length} 项 · 重复 URL 自动跳过</p>}
        <div className="actions">
          <button type="button" disabled={busy} onClick={close}>{native ? "取消" : "关闭"}</button>
          {native && <button type="button" className="primary" disabled={locked || !chosen.length}
            onClick={() => void submit()}>{busy ? "正在导入…" : "导入所选书签"}</button>}
        </div>
      </footer>
    </Modal>
  );
}
