import { createElement, useEffect, useMemo, useState, type ReactNode } from "react";
import DOMPurify from "dompurify";
import ImageViewer from "./ImageViewer";
import useImageActions from "./useImageActions";
import { command } from "../lib/api";

function BugStepImage({ bugId, historyActionId, source, alt, onPreview }: {
  bugId: string;
  historyActionId?: string;
  source: string;
  alt: string;
  onPreview: (src: string) => void;
}) {
  const [preview, setPreview] = useState("");
  const imageActions = useImageActions();
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const imageLabel = historyActionId ? "历史记录图片" : "复现步骤图片";
  useEffect(() => {
    let active = true;
    setPreview("");
    setError("");
    // 原始 src 只交给后端核验附件引用，浏览器不直接访问 HTML 中的地址。
    command<{ dataUrl: string }>(
      historyActionId ? "zentao_bug_history_image_read" : "zentao_bug_image_read",
      historyActionId ? { bugId, actionId: historyActionId, source } : { bugId, source },
    )
      .then(({ dataUrl }) => {
        if (!active) return;
        if (!/^data:image\/(png|jpeg|gif|webp);base64,[a-z0-9+/=]+$/i.test(dataUrl))
          throw new Error("图片返回格式无效");
        setPreview(dataUrl);
      })
      .catch(cause => {
        if (active) setError(cause instanceof Error ? cause.message : "图片加载失败");
      });
    return () => { active = false; };
  }, [bugId, historyActionId, source, retry]);
  return <span className="bugs-step-image">
    {preview ? <button type="button" className="bugs-step-image-button" aria-label={`放大查看${imageLabel}`} onClick={() => onPreview(preview)}>
      <img onContextMenu={imageActions.onContextMenu} src={preview} alt={alt || `BUG ${imageLabel}`} />
    </button> : <span className="bugs-step-image-placeholder" role="status">
      <span>{error || `正在加载${imageLabel}…`}</span>
      {error && <button type="button" onClick={() => setRetry(value => value + 1)}>重试加载</button>}
    </span>}
    {imageActions.overlay}
  </span>;
}

export default function BugSteps({ bugId, value, historyActionId }: { bugId: string; value: string; historyActionId?: string }) {
  const [preview, setPreview] = useState("");
  useEffect(() => { setPreview(""); }, [bugId, value, historyActionId]);
  const content = useMemo(() => {
    const fragment = DOMPurify.sanitize(value, {
      ALLOWED_TAGS: ["p", "br", "div", "span", "b", "strong", "em", "i", "u", "s", "ul", "ol", "li", "pre", "code", "blockquote", "table", "thead", "tbody", "tr", "td", "th", "h1", "h2", "h3", "h4", "img"],
      ALLOWED_ATTR: ["src", "alt", "colspan", "rowspan"],
      ALLOW_DATA_ATTR: false,
      RETURN_DOM_FRAGMENT: true,
    });
    function render(node: Node, key: string): ReactNode {
      if (node.nodeType === Node.TEXT_NODE) return node.textContent;
      if (!(node instanceof Element)) return null;
      const tag = node.tagName.toLowerCase();
      if (tag === "img") return <BugStepImage key={key} bugId={bugId} historyActionId={historyActionId} source={node.getAttribute("src") || ""} alt={node.getAttribute("alt") || ""}
        onPreview={setPreview} />;
      const attributes: { key: string; colSpan?: number; rowSpan?: number } = { key };
      if (tag === "td" || tag === "th") {
        for (const [html, prop] of [["colspan", "colSpan"], ["rowspan", "rowSpan"]] as const) {
          const count = Number(node.getAttribute(html));
          if (Number.isInteger(count) && count > 0 && count <= 100) attributes[prop] = count;
        }
      }
      return createElement(tag, attributes, tag === "br" ? undefined : [...node.childNodes].map((child, index) => render(child, `${key}-${index}`)));
    }
    // 图片交由原生按正文或指定历史记录核验；预览不修改原始 HTML。
    return [...fragment.childNodes].map((node, index) => render(node, String(index)));
  }, [bugId, value, historyActionId]);
  return <>
    <div className={historyActionId ? "bug-history-comment" : "bugs-steps"}>{content.length ? content : historyActionId ? null : <p className="muted">暂无复现步骤</p>}</div>
    {preview && <ImageViewer src={preview} title={historyActionId ? "历史记录图片预览" : "复现步骤图片预览"} alt={historyActionId ? "BUG 历史记录图片放大预览" : "BUG 复现步骤放大预览"} onClose={() => setPreview("")} />}
  </>;
}
