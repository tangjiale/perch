import { useMemo, useState, type FormEvent } from "react";
import { Copy, FileText, Pencil, Plus, RefreshCw, Search, Star, Trash2, WandSparkles } from "lucide-react";
import Modal from "../components/Modal";
import GlassSelect from "../components/GlassSelect";
import { api, native } from "../lib/api";
import { filterPrompts, parsePromptTags, PROMPT_LIMIT, promptKindLabels, type PromptEntry, type PromptKind } from "../lib/prompt-optimizer";
import "./prompt-library.css";

const message = (error: unknown) => error instanceof Error ? error.message : String(error);

export function PromptEditor({ entry, initial, onClose, onSaved }: {
  entry?: PromptEntry;
  initial?: { content: string; kind: PromptKind };
  onClose: () => void;
  onSaved: (entry: PromptEntry) => void;
}) {
  const [id] = useState(() => entry?.id ?? crypto.randomUUID());
  const [title, setTitle] = useState(entry?.title ?? "");
  const [content, setContent] = useState(entry?.content ?? initial?.content ?? "");
  const [kind, setKind] = useState<PromptKind>(entry?.kind ?? initial?.kind ?? "system");
  const [tags, setTags] = useState(entry?.tags.join("，") ?? "");
  const [favorite, setFavorite] = useState(entry?.favorite ?? false);
  const [version, setVersion] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const selectedVersion = entry?.versions.find((item) => item.id === version);
  async function save(event: FormEvent) {
    event.preventDefault();
    if (busy) return;
    const parsedTags = parsePromptTags(tags);
    if (!native) { setError("请在桌面应用中保存提示词。"); return; }
    if (!title.trim() || title.trim().length > 120) { setError("请输入 1–120 字的标题。"); return; }
    if (!content.trim() || content.length > PROMPT_LIMIT) { setError("请输入 1–30000 字的提示词正文。"); return; }
    if (parsedTags.length > 12 || parsedTags.some((tag) => tag.length > 30)) { setError("最多添加 12 个标签，每个标签最多 30 字。"); return; }
    setBusy(true); setError("");
    try {
      const saved = await api.promptSave({ ...entry, id, title: title.trim(), content, kind, tags: parsedTags, favorite, versions: entry?.versions ?? [] });
      onSaved(saved);
    } catch (cause) { setError(message(cause)); }
    finally { setBusy(false); }
  }
  return <Modal title={entry ? "编辑提示词" : "保存提示词"} onClose={onClose} wide busy={busy} className="prompt-editor">
    <form onSubmit={save}>
      <label>标题<input autoFocus value={title} maxLength={120} onChange={(event) => setTitle(event.target.value)} placeholder="为提示词取一个方便查找的名字" disabled={busy} /></label>
      <div className="prompt-editor-meta">
        <label>类型<GlassSelect value={kind} options={Object.entries(promptKindLabels).map(([value, label]) => ({ value, label }))} onValueChange={(value) => setKind(value as PromptKind)} disabled={busy} /></label>
        <label>标签<input value={tags} onChange={(event) => setTags(event.target.value)} placeholder="用逗号分隔，如 写作，工作" disabled={busy} /></label>
        <label className="prompt-check"><input type="checkbox" checked={favorite} onChange={(event) => setFavorite(event.target.checked)} disabled={busy} />收藏</label>
      </div>
      <label>提示词正文<textarea value={content} maxLength={PROMPT_LIMIT} onChange={(event) => setContent(event.target.value)} rows={12} disabled={busy} /></label>
      <div className="prompt-editor-count">{content.length.toLocaleString()} / 30,000 字</div>
      {!!entry?.versions.length && <details className="prompt-version-history"><summary>版本记录 · {entry.versions.length} 个版本</summary>
        <div className="prompt-version-controls"><GlassSelect value={version} placeholder="选择版本预览" options={[...entry.versions].reverse().map((item, index) => ({ value: item.id, label: `${new Date(item.createdAt).toLocaleString("zh-CN")} · 版本 ${entry.versions.length - index}` }))} onValueChange={setVersion} disabled={busy} />
          <button type="button" disabled={!selectedVersion || busy} onClick={() => { if (selectedVersion) setContent(selectedVersion.content); }}>还原到编辑区</button></div>
        {selectedVersion && <pre>{selectedVersion.content}</pre>}<p>还原后点击保存生成新版本，最多保留最近 30 个版本。</p>
      </details>}
      {!native && <p className="prompt-library-notice">提示词库需要在桌面应用中使用。</p>}
      {error && <p className="prompt-library-error" role="alert">{error}</p>}
      <footer><button type="button" onClick={onClose} disabled={busy}>取消</button><button type="submit" className="primary" disabled={busy || !native}>{busy ? "正在保存…" : "保存提示词"}</button></footer>
    </form>
  </Modal>;
}

export default function PromptLibrary({ entries, loading, error, onRetry, onSaved, onDeleted, onUse, notify }: {
  entries: PromptEntry[]; loading: boolean; error: string; onRetry: () => void;
  onSaved: (entry: PromptEntry) => void; onDeleted: (id: string) => void;
  onUse: (entry: PromptEntry) => void; notify: (text: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [tag, setTag] = useState("");
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [editor, setEditor] = useState<PromptEntry | "new" | null>(null);
  const [deleting, setDeleting] = useState<PromptEntry | null>(null);
  const [busyId, setBusyId] = useState("");
  const [deleteError, setDeleteError] = useState("");
  const tags = useMemo(() => [...new Set(entries.flatMap((entry) => entry.tags))].sort((a, b) => a.localeCompare(b, "zh-CN")), [entries]);
  const visible = useMemo(() => filterPrompts(entries, query, tag, favoritesOnly), [entries, query, tag, favoritesOnly]);
  async function toggleFavorite(entry: PromptEntry) {
    if (busyId || !native) return;
    setBusyId(entry.id);
    try { onSaved(await api.promptSave({ ...entry, favorite: !entry.favorite })); }
    catch (cause) { notify(message(cause)); }
    finally { setBusyId(""); }
  }
  async function remove() {
    if (!deleting || busyId || !native) return;
    setBusyId(deleting.id); setDeleteError("");
    try { await api.promptDelete(deleting.id, deleting.revision); onDeleted(deleting.id); setDeleting(null); }
    catch (cause) { setDeleteError(message(cause)); }
    finally { setBusyId(""); }
  }
  return <section className="prompt-library" aria-label="我的提示词">
    <div className="prompt-library-toolbar">
      <label className="prompt-library-search"><Search size={16} /><input aria-label="搜索提示词" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索标题、正文或标签" /></label>
      <GlassSelect value={tag} onValueChange={setTag} options={[{ value: "", label: "全部标签" }, ...tags.map((value) => ({ value, label: value }))]} aria-label="按标签筛选" />
      <button className={favoritesOnly ? "active" : ""} aria-pressed={favoritesOnly} onClick={() => setFavoritesOnly(!favoritesOnly)}><Star size={15} />只看收藏</button>
      <button className="icon-button" title="刷新提示词库" aria-label="刷新提示词库" disabled={!native || loading || !!busyId} onClick={onRetry}><RefreshCw size={15} /></button>
      <button className="primary" onClick={() => setEditor("new")} disabled={!native || loading}><Plus size={16} />新建提示词</button>
    </div>
    {!native && <p className="prompt-library-notice">在桌面应用中管理和保存自己的提示词。</p>}
    {error && <div className="prompt-library-error" role="alert">{error}<button onClick={onRetry} disabled={loading}>重新加载</button></div>}
    <div className="prompt-library-summary">{visible.length} 条提示词{visible.length !== entries.length ? ` · 共 ${entries.length} 条` : ""}</div>
    {loading && !entries.length ? <div className="prompt-library-empty">正在加载提示词…</div> : !visible.length ? <div className="prompt-library-empty"><FileText size={28} /><strong>{entries.length ? "没有符合条件的提示词" : "把好用的提示词留在这里"}</strong><span>{entries.length ? "试试其他关键词或筛选条件。" : "新建自己的提示词，或在优化后保存到提示词库。"}</span></div> : <div className="prompt-library-list">
      {visible.map((entry) => <article key={entry.id}>
        <button className="prompt-library-star icon-button" title={entry.favorite ? "取消收藏" : "收藏"} aria-label={`${entry.favorite ? "取消收藏" : "收藏"}：${entry.title}`} aria-pressed={entry.favorite} disabled={!!busyId || !native} onClick={() => void toggleFavorite(entry)}><Star size={17} fill={entry.favorite ? "currentColor" : "none"} /></button>
        <div className="prompt-library-entry"><div className="prompt-library-entry-title"><strong>{entry.title}</strong><span>{promptKindLabels[entry.kind]}</span></div><p>{entry.content}</p><div className="prompt-library-tags">{entry.tags.map((item) => <button key={item} onClick={() => setTag(item)}>{item}</button>)}{entry.updatedAt && <time>{new Date(entry.updatedAt).toLocaleDateString("zh-CN")}</time>}</div></div>
        <div className="prompt-library-actions"><button title="载入优化器" onClick={() => onUse(entry)}><WandSparkles size={15} />优化</button><button className="icon-button" title="复制正文" onClick={() => void navigator.clipboard.writeText(entry.content).then(() => notify("已复制提示词"), (cause) => notify(`复制失败：${message(cause)}`))}><Copy size={15} /></button><button className="icon-button" title="编辑提示词" disabled={!native || !!busyId} onClick={() => setEditor(entry)}><Pencil size={15} /></button><button className="icon-button" title="删除提示词" disabled={!native || !!busyId} onClick={() => { setDeleting(entry); setDeleteError(""); }}><Trash2 size={15} /></button></div>
      </article>)}
    </div>}
    {editor && <PromptEditor entry={editor === "new" ? undefined : editor} onClose={() => setEditor(null)} onSaved={(entry) => { onSaved(entry); setEditor(null); notify("提示词已保存"); }} />}
    {deleting && <Modal title="删除提示词" className="prompt-editor" busy={!!busyId} onClose={() => setDeleting(null)}><p>确定删除「{deleting.title}」及其版本记录？此操作无法撤销。</p>{deleteError && <p className="prompt-library-error" role="alert">{deleteError}</p>}<footer><button disabled={!!busyId} onClick={() => setDeleting(null)}>取消</button><button className="danger" disabled={!!busyId} onClick={() => void remove()}>{busyId ? "正在删除…" : "确认删除"}</button></footer></Modal>}
  </section>;
}
