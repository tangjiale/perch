import { createContext, useContext, useEffect, useRef, useState } from "react";
import { EditorContent, NodeViewWrapper, ReactNodeViewRenderer, useEditor, type NodeViewProps } from "@tiptap/react";
import { createPortal } from "react-dom";
import Modal from "./Modal";
import StarterKit from "@tiptap/starter-kit";
import Image from "@tiptap/extension-image";
import { TableKit } from "@tiptap/extension-table";
import { Bold, Italic, Underline, List, ListOrdered, ImagePlus, Trash2, Undo2, Redo2 } from "lucide-react";
import { command } from "../lib/api";
import { taskRichHtml } from "../lib/task-rich-text";
import type { Task } from "../lib/types";
import "./task-rich-text.css";

const MediaContext = createContext<{ taskId?: string; previews: Record<string, string>; onPreview?: (src: string) => void }>({ previews: {} });
const safeImageData = (value: string) => /^data:image\/(png|jpeg|gif|webp);base64,[a-z0-9+/=]+$/i.test(value);

function TaskImageView({ node, selected, editor, getPos }: NodeViewProps) {
  const { taskId, previews, onPreview } = useContext(MediaContext);
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
    if (!taskId) { setError("此图片需要已保存的禅道任务才能读取"); return; }
    command<{ dataUrl: string }>("zentao_task_image_read", { taskId, source })
      .then(({ dataUrl }) => { if (active) { if (!safeImageData(dataUrl)) throw Error("图片返回格式无效"); setPreview(dataUrl); } })
      .catch(cause => { if (active) setError(cause instanceof Error ? cause.message : "图片加载失败"); });
    return () => { active = false; };
  }, [taskId, source, previews, retry]);
  return <NodeViewWrapper className={`task-rich-image ${selected ? "is-selected" : ""}`} contentEditable={false}>
    {preview ? <img src={preview} alt={node.attrs.alt || "任务图片"} draggable={false} role="button" tabIndex={0} aria-label="放大查看任务图片" title="点击放大" onClick={openPreview} onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); openPreview(); } }} /> : <div className="task-rich-image-placeholder">
      <span>{error || "正在加载图片…"}</span>
      {error && <button type="button" onClick={() => setRetry(value => value + 1)}>重试加载</button>}
    </div>}
  </NodeViewWrapper>;
}
const TaskImage = Image.extend({ addNodeView() { return ReactNodeViewRenderer(TaskImageView); } });

export default function TaskRichTextEditor({ task, value, disabled, remoteDraft, onChange, onBusyChange }: {
  task: Task; value: string; disabled: boolean; remoteDraft: boolean;
  onChange: (html: string) => void; onBusyChange: (busy: boolean) => void;
}) {
  const [imagePreview, setImagePreview] = useState("");
  const [originalSize, setOriginalSize] = useState(false);
  const [error, setError] = useState("");
  const [uploading, setUploading] = useState(false);
  const [previews, setPreviews] = useState<Record<string,string>>({});
  const fileInput = useRef<HTMLInputElement>(null);
  const inFlight = useRef(false);
  const uploadRef = useRef<(file: File) => void>(() => {});
  const editor = useEditor({
    shouldRerenderOnTransaction: true,
    extensions: [StarterKit.configure({ link: { openOnClick: false }, heading: { levels: [1,2,3] } }), TaskImage.configure({ allowBase64: true }), TableKit],
    content: taskRichHtml(value),
    editable: !disabled,
    onUpdate: ({ editor }) => onChange(editor.getHTML()),
    editorProps: {
      attributes: { role: "textbox", "aria-label": "任务备注富文本", "aria-multiline": "true" },
      transformPastedHTML: taskRichHtml,
      handlePaste: (_view, event) => {
        const files = [...(event.clipboardData?.files ?? [])];
        if (!files.length) return false;
        event.preventDefault(); uploadRef.current(files[0]); return true;
      },
      handleDrop: (_view, event) => {
        const files = [...(event.dataTransfer?.files ?? [])];
        if (!files.length) return false;
        event.preventDefault(); uploadRef.current(files[0]); return true;
      },
    },
  });
  // 只切换交互状态，不触发备注序列化；否则未编辑的禅道 HTML 也会被当成修改。
  useEffect(() => { editor?.setEditable(!disabled && !uploading, false); }, [editor, disabled, uploading]);
  async function upload(file: File) {
    if (!editor || inFlight.current || disabled) return;
    if (remoteDraft) { setError("请先保存禅道执行，再上传图片"); return; }
    if (!/^image\/(png|jpeg|gif|webp)$/.test(file.type) || file.size > 5 * 1024 * 1024) {
      setError("请选择不超过 5 MB 的 PNG、JPG、GIF 或 WebP 图片"); return;
    }
    inFlight.current = true; setUploading(true); onBusyChange(true); setError("");
    try {
      const dataUrl = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader(); reader.onload = () => resolve(String(reader.result)); reader.onerror = () => reject(Error("无法读取图片")); reader.readAsDataURL(file);
      });
      if (!safeImageData(dataUrl)) throw Error("图片数据无效");
      let src = dataUrl;
      if (task.source === "zentao") {
        const result = await command<{ source: string }>("zentao_task_image_upload", { taskId: task.id, name: file.name, dataUrl });
        src = result.source;
        setPreviews(previous => ({ ...previous, [src]: dataUrl }));
      }
      editor.chain().focus().setImage({ src, alt: file.name }).run();
    } catch (cause) { setError(cause instanceof Error ? cause.message : "图片上传失败"); }
    finally { inFlight.current = false; setUploading(false); onBusyChange(false); }
  }
  uploadRef.current = file => { void upload(file); };
  if (!editor) return <p className="muted">正在准备编辑器…</p>;
  return <div className="task-rich-text">
    <div className="task-rich-toolbar" role="toolbar" aria-label="备注格式">
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
      <button type="button" title="插入图片" aria-label="插入图片" disabled={disabled || uploading || remoteDraft} onClick={() => fileInput.current?.click()}><ImagePlus size={16}/></button>
      <button type="button" title="删除选中的图片" aria-label="删除选中的图片" disabled={disabled || uploading || !editor.isActive("image")} onClick={() => editor.chain().focus().deleteSelection().run()}><Trash2 size={15}/></button>
      {uploading && <span className="muted">正在上传图片…</span>}
    </div>
    <input ref={fileInput} type="file" hidden accept="image/png,image/jpeg,image/gif,image/webp" onChange={event => { const file = event.target.files?.[0]; if (file) void upload(file); event.target.value = ""; }}/>
    <MediaContext.Provider value={{ taskId: task.source === "zentao" ? task.id : undefined, previews, onPreview: src => { setOriginalSize(false); setImagePreview(src); } }}>
      <EditorContent editor={editor}/>
    </MediaContext.Provider>
    <p className="task-rich-hint">可粘贴或插入图片，点击图片可放大，关闭预览后可删除选中的图片。{task.source === "zentao" ? "保存任务后同步正文；上传成功但取消编辑的文件仍保留在禅道。" : remoteDraft ? "请先保存禅道执行，再上传图片。" : "图片随任务保存在本地。"}</p>
    {imagePreview && createPortal(<div className="task-image-preview-layer" onClick={event => event.stopPropagation()}>
      <Modal title="图片预览" wide onClose={() => setImagePreview("")}>
        <div className="task-image-preview-actions"><button type="button" onClick={() => setOriginalSize(value => !value)}>{originalSize ? "适应窗口" : "原始尺寸"}</button><span className="muted">按 Esc 或点击空白处关闭</span></div>
        <div className={`task-image-preview-stage ${originalSize ? "original-size" : ""}`} onClick={event => { if (event.target === event.currentTarget) setImagePreview(""); }}>
          <img src={imagePreview} alt="任务图片放大预览" />
        </div>
      </Modal>
    </div>, document.body)}
    {error && <p role="alert" className="error">{error}</p>}
  </div>;
}
