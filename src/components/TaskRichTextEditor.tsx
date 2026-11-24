import { createContext, useContext, useEffect, useRef, useState } from "react";
import { NodeSelection, TextSelection } from "@tiptap/pm/state";
import { EditorContent, NodeViewWrapper, ReactNodeViewRenderer, useEditor, type JSONContent, type NodeViewProps } from "@tiptap/react";
import ImageViewer from "./ImageViewer";
import useImageActions from "./useImageActions";
import StarterKit from "@tiptap/starter-kit";
import Image from "@tiptap/extension-image";
import { TableKit } from "@tiptap/extension-table";
import { Bold, Italic, Underline, List, ListOrdered, ImagePlus, Trash2, Undo2, Redo2 } from "lucide-react";
import { command } from "../lib/api";
import { taskRichHtml } from "../lib/task-rich-text";
import type { Task } from "../lib/types";
import "./task-rich-text.css";

const MediaContext = createContext<{ taskId?: string; bugId?: string; bugSteps?: boolean; previews: Record<string, string>; onPreview?: (src: string) => void }>({ previews: {} });
const safeImageData = (value: string) => /^data:image\/(png|jpeg|gif|webp);base64,[a-z0-9+/=]+$/i.test(value);
const maxImageBytes = 5 * 1024 * 1024;

function draftImageData(value: string): string | undefined {
  if (value.length > Math.ceil(maxImageBytes / 3) * 4 + "data:image/jpeg;base64,".length) return;
  const match = /^data:image\/(png|jpeg|gif|webp);base64,([a-z0-9+/]+={0,2})$/i.exec(value);
  if (!match || match[0].length !== value.length) return;
  const [, rawFormat, encoded] = match;
  const padding = encoded.endsWith("==") ? 2 : encoded.endsWith("=") ? 1 : 0;
  if (encoded.length % 4 || encoded.length / 4 * 3 - padding > maxImageBytes) return;

  const format = rawFormat.toLowerCase() as "png" | "jpeg" | "gif" | "webp";
  const header = atob(encoded.slice(0, 16));
  const matchesFormat = {
    png: header.startsWith("\x89PNG\r\n\x1a\n"),
    jpeg: header.startsWith("\xff\xd8\xff"),
    gif: header.startsWith("GIF87a") || header.startsWith("GIF89a"),
    webp: header.startsWith("RIFF") && header.slice(8, 12) === "WEBP",
  }[format];
  return matchesFormat ? `data:image/${format};base64,${encoded}` : undefined;
}

function draftBugHtml(value: string, onImageRejected?: () => void): string {
  // 在惰性模板中移除外部图片，再把 HTML 交给编辑器，避免浏览器请求原始 src。
  const template = document.createElement("template");
  template.innerHTML = value;
  const images = template.content.querySelectorAll("img");
  if (!images.length) return taskRichHtml(value);
  let rejected = false;
  for (const image of images) {
    const src = draftImageData(image.getAttribute("src") ?? "");
    if (src) {
      image.setAttribute("src", src);
      image.removeAttribute("srcset");
    } else {
      image.remove();
      rejected = true;
    }
  }
  if (rejected) onImageRejected?.();
  return taskRichHtml(template.innerHTML);
}

function TaskImageView({ node, selected, editor, getPos }: NodeViewProps) {
  const imageActions = useImageActions();
  const { taskId, bugId, bugSteps, previews, onPreview } = useContext(MediaContext);
  const openPreview = () => {
    const pos = getPos();
    if (typeof pos === "number") editor.commands.setNodeSelection(pos);
    onPreview?.(preview);
  };
  const source = String(node.attrs.src ?? "");
  const [preview, setPreview] = useState("");
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let active = true;
    setPreview(""); setError("");
    if (safeImageData(source)) { setPreview(source); return; }
    if (previews[source]) { setPreview(previews[source]); return; }
    if (!taskId && !bugId) { setError("此图片需要已保存的禅道记录才能读取"); return; }
    command<{ dataUrl: string }>(bugId ? "zentao_bug_image_read" : "zentao_task_image_read", bugId ? { bugId, source } : { taskId, source })
      .then(({ dataUrl }) => { if (active) { if (!safeImageData(dataUrl)) throw Error("图片返回格式无效"); setPreview(dataUrl); } })
      .catch(cause => { if (active) setError(cause instanceof Error ? cause.message : "图片加载失败"); });
    return () => { active = false; };
  }, [taskId, bugId, source, previews, retry]);
  return <NodeViewWrapper className={`task-rich-image ${selected ? "is-selected" : ""}`} contentEditable={false}>
    {preview ? <img onContextMenu={imageActions.onContextMenu} src={preview} alt={node.attrs.alt || (bugSteps ? "BUG 复现步骤截图" : "任务图片")} draggable={false} role="button" tabIndex={0} aria-label={bugSteps ? "放大查看复现步骤图片" : "放大查看任务图片"} title="点击放大，右键复制或下载图片" onClick={openPreview} onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); openPreview(); } }} /> : <div className="task-rich-image-placeholder">
      <span>{error || "正在加载图片…"}</span>
      {error && <button type="button" onClick={() => setRetry(value => value + 1)}>重试加载</button>}
    </div>}
    {imageActions.overlay}
  </NodeViewWrapper>;
}
const TaskImage = Image.extend({ addNodeView() { return ReactNodeViewRenderer(TaskImageView); } });

type EditorProps = {
  value: string; disabled: boolean;
  onChange: (html: string) => void; onBusyChange: (busy: boolean) => void;
};
type TaskEditorProps = EditorProps & { task: Task; remoteDraft: boolean };
type BugDraftEditorProps = EditorProps & { bugDraft: true; initialStepsFormat?: "html" };

export default function TaskRichTextEditor(props: TaskEditorProps | BugDraftEditorProps) {
  return <RichTextEditor {...props} />;
}

export function BugRichTextEditor(props: {
  bugId: string; value: string; disabled: boolean; onChange: (html: string) => void;
}) {
  return <RichTextEditor {...props} remoteDraft={false} onBusyChange={() => {}} />;
}

function RichTextEditor({ task, bugId, bugDraft, initialStepsFormat, value, disabled, remoteDraft, onChange, onBusyChange }: EditorProps & {
  task?: Task; bugId?: string; bugDraft?: true; initialStepsFormat?: "html"; remoteDraft?: boolean;
}) {
  const bugSteps = bugDraft || Boolean(bugId);
  const [initialContent] = useState<string | JSONContent>(() => {
    if (!bugDraft) return taskRichHtml(value);
    if (initialStepsFormat === "html") return draftBugHtml(value);
    // 旧本地 BUG 保存的是纯文本，尖括号等源码字符须原样显示。
    return {
      type: "doc",
      content: value.split(/\r\n?|\n/).map(text => ({
        type: "paragraph",
        content: text ? [{ type: "text", text }] : [],
      })),
    };
  });
  const [imagePreview, setImagePreview] = useState("");
  const [error, setError] = useState("");
  const [uploading, setUploading] = useState(false);
  const [previews, setPreviews] = useState<Record<string,string>>({});
  const fileInput = useRef<HTMLInputElement>(null);
  const readerRef = useRef<FileReader | null>(null);
  const mounted = useRef(true);
  const inFlight = useRef(false);
  const uploadRef = useRef<(file: File, position?: number) => void>(() => {});
  const editor = useEditor({
    shouldRerenderOnTransaction: true,
    extensions: [StarterKit.configure({ link: { openOnClick: false }, heading: { levels: [1,2,3] } }), TaskImage.configure({ allowBase64: true }), TableKit],
    content: initialContent,
    editable: !disabled,
    onUpdate: ({ editor, transaction }) => {
      // 选中图片时插件可能补齐尾段；只有用户实际修改正文才回写原始 HTML。
      if (transaction.docChanged) onChange(editor.getHTML());
    },
    editorProps: {
      attributes: { role: "textbox", "aria-label": bugSteps ? "BUG 复现步骤富文本" : "任务备注富文本", "aria-multiline": "true" },
      handleDOMEvents: {
        click: (view, event) => {
          if (!view.editable || event.button !== 0 || event.detail !== 1 || event.shiftKey || event.ctrlKey || event.metaKey || event.altKey) return false;
          const { doc, selection } = view.state;
          const first = doc.firstChild;
          const empty = doc.childCount === 1 && first?.isTextblock && first.content.size === 0;
          if (event.target !== view.dom && !empty) return false;

          const anchor = view.dom.ownerDocument.getSelection()?.anchorNode;
          if (view.hasFocus() && anchor && anchor !== view.dom && view.dom.contains(anchor)) return false;

          // 点击正文空白但 WebView 没给出有效光标时，显式建立正文选区。
          const hit = view.posAtCoords({ left: event.clientX, top: event.clientY });
          const position = doc.resolve(hit?.pos ?? doc.content.size);
          const caret = TextSelection.between(position, position, -1);
          if (!selection.eq(caret)) view.dispatch(view.state.tr.setSelection(caret));
          view.focus();
          return true;
        },
      },
      transformPastedHTML: bugDraft
        ? html => draftBugHtml(html, () => setError("已忽略外部图片或无效、超过 5 MB 的图片，请选择本地图片文件。"))
        : taskRichHtml,
      handlePaste: (_view, event) => {
        const files = [...(event.clipboardData?.files ?? [])];
        if (!files.length) return false;
        event.preventDefault(); uploadRef.current(files[0]); return true;
      },
      handleDrop: (view, event) => {
        const files = [...(event.dataTransfer?.files ?? [])];
        if (!files.length) return false;
        const position = bugDraft ? view.posAtCoords({ left: event.clientX, top: event.clientY })?.pos : undefined;
        event.preventDefault(); uploadRef.current(files[0], position); return true;
      },
    },
  });
  // 只切换交互状态，不触发备注序列化；否则未编辑的禅道 HTML 也会被当成修改。
  useEffect(() => { editor?.setEditable(!disabled && !uploading, false); }, [editor, disabled, uploading]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      readerRef.current?.abort();
    };
  }, []);
  async function upload(file: File, position?: number) {
    if (!editor || inFlight.current || disabled) return;
    if (!task && !bugDraft) { setError("新增 BUG 图片请在禅道中上传；可在此编辑文字和预览已有截图"); return; }
    if (remoteDraft) { setError("请先保存禅道执行，再上传图片"); return; }
    if (!/^image\/(png|jpeg|gif|webp)$/.test(file.type) || file.size > maxImageBytes) {
      setError("请选择不超过 5 MB 的 PNG、JPG、GIF 或 WebP 图片"); return;
    }
    inFlight.current = true; setUploading(true); onBusyChange(true); setError("");
    try {
      const dataUrl = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        readerRef.current = reader;
        reader.onload = () => resolve(String(reader.result));
        reader.onerror = () => reject(Error("无法读取图片"));
        reader.onabort = () => reject(Error("读取图片已取消"));
        reader.readAsDataURL(file);
      });
      let src = bugDraft ? draftImageData(dataUrl) : safeImageData(dataUrl) ? dataUrl : undefined;
      if (!src) throw Error("图片数据无效");
      if (!mounted.current || editor.isDestroyed) return;
      if (!bugDraft && task?.source === "zentao") {
        const result = await command<{ source: string }>("zentao_task_image_upload", { taskId: task.id, name: file.name, dataUrl });
        if (!mounted.current || editor.isDestroyed) return;
        src = result.source;
        setPreviews(previous => ({ ...previous, [result.source]: dataUrl }));
      }
      const attrs = { src, alt: file.name };
      const selection = editor.state.selection;
      if (bugDraft && selection instanceof NodeSelection && selection.node.type.name === "image") {
        position ??= selection.to;
      }
      const chain = editor.chain().focus();
      const inserted = position === undefined
        ? chain.setImage(attrs).run()
        : chain.insertContentAt(position, { type: "image", attrs }).run();
      if (bugDraft && !inserted) throw Error("无法插入图片，请将光标移到正文后重试");
    } catch (cause) {
      if (mounted.current) setError(cause instanceof Error ? cause.message : bugDraft ? "读取图片失败" : "图片上传失败");
    } finally {
      readerRef.current = null;
      inFlight.current = false;
      if (mounted.current) { setUploading(false); onBusyChange(false); }
    }
  }
  uploadRef.current = (file, position) => { void upload(file, position); };
  if (!editor) return <p className="muted">正在准备编辑器…</p>;
  const { childCount, firstChild } = editor.state.doc;
  const emptyParagraph = childCount === 1 && firstChild?.type.name === "paragraph" && firstChild.content.size === 0;
  return <div className="task-rich-text">
    <div className="task-rich-toolbar" role="toolbar" aria-label={bugSteps ? "复现步骤格式" : "备注格式"}>
      {[
        ["加粗", Bold, () => editor.chain().focus().toggleBold().run()],
        ["斜体", Italic, () => editor.chain().focus().toggleItalic().run()],
        ["下划线", Underline, () => editor.chain().focus().toggleUnderline().run()],
        ["无序列表", List, () => editor.chain().focus().toggleBulletList().run()],
        ["有序列表", ListOrdered, () => editor.chain().focus().toggleOrderedList().run()],
        ["撤销", Undo2, () => editor.chain().focus().undo().run()],
        ["重做", Redo2, () => editor.chain().focus().redo().run()],
      ].map(([label, Icon, action]) => {
        const ButtonIcon = Icon as typeof Bold;
        return <button type="button" key={String(label)} title={String(label)} aria-label={String(label)} disabled={disabled || uploading} onClick={action as () => void}><ButtonIcon size={15}/></button>;
      })}
      {(task || bugDraft) && <button type="button" title="插入图片" aria-label="插入图片" disabled={disabled || uploading || remoteDraft} onClick={() => fileInput.current?.click()}><ImagePlus size={16}/></button>}
      <button type="button" title="删除选中的图片" aria-label="删除选中的图片" disabled={disabled || uploading || !editor.isActive("image")} onClick={() => editor.chain().focus().deleteSelection().run()}><Trash2 className="danger" size={15}/></button>
      {uploading && <span className="muted" role="status">{bugDraft ? "正在读取图片…" : "正在上传图片…"}</span>}
    </div>
    <input ref={fileInput} type="file" hidden accept="image/png,image/jpeg,image/gif,image/webp" onChange={event => { const file = event.target.files?.[0]; if (file) void upload(file); event.target.value = ""; }}/>
    <MediaContext.Provider value={{ taskId: !bugDraft && task?.source === "zentao" ? task.id : undefined, bugId, bugSteps, previews, onPreview: setImagePreview }}>
      <EditorContent className="task-rich-body" data-empty={emptyParagraph || undefined} editor={editor}/>
    </MediaContext.Provider>
    <p className="task-rich-hint">{bugDraft
      ? "建议填写步骤、实际结果和预期结果。可粘贴、拖入或插入图片，单张不超过 5 MB，最多 8 张，合计不超过 12 MB。点击图片可放大。"
      : bugId ? "支持文字排版和已有截图预览，新增图片请在禅道中上传；保存后同步复现步骤。" : <>可粘贴或插入图片，点击图片可放大，关闭预览后可删除选中的图片。{task?.source === "zentao" ? "保存任务后同步正文；上传成功但取消编辑的文件仍保留在禅道。" : remoteDraft ? "请先保存禅道执行，再上传图片。" : "图片随任务保存在本地。"}</>}</p>
    {imagePreview && <ImageViewer src={imagePreview} alt={bugSteps ? "BUG 复现步骤放大预览" : "任务图片放大预览"} onClose={() => setImagePreview("")} />}
    {error && <p role="alert" className="error">{error}</p>}
  </div>;
}
