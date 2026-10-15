import { createElement, useEffect, useMemo, useState, type ReactNode } from "react";
import DOMPurify from "dompurify";
import ImageViewer from "./ImageViewer";
import useImageActions from "./useImageActions";
import { command } from "../lib/api";

function BugStepImage({ bugId, source, alt, onPreview }: {
  bugId: string;
  source: string;
  alt: string;
  onPreview: (src: string) => void;
}) {
  const [preview, setPreview] = useState("");
  const imageActions = useImageActions();
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let active = true;
    setPreview("");
    setError("");
    // 原始 src 只交给后端核验附件引用，浏览器不直接访问 HTML 中的地址。
    command<{ dataUrl: string }>("zentao_bug_image_read", { bugId, source })
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
  }, [bugId, source, retry]);
  return <span className="bugs-step-image">
    {preview ? <button type="button" className="bugs-step-image-button" aria-label="放大查看复现步骤图片" onClick={() => onPreview(preview)}>
      <img onContextMenu={imageActions.onContextMenu} src={preview} alt={alt || "BUG 复现步骤截图"} />
    </button> : <span className="bugs-step-image-placeholder" role="status">
      <span>{error || "正在加载复现步骤图片…"}</span>
      {error && <button type="button" onClick={() => setRetry(value => value + 1)}>重试加载</button>}
    </span>}
    {imageActions.overlay}
  </span>;
}

export default function BugSteps({ bugId, value }: { bugId: string; value: string }) {
  const [preview, setPreview] = useState("");
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
      if (tag === "img") return <BugStepImage key={key} bugId={bugId} source={node.getAttribute("src") || ""} alt={node.getAttribute("alt") || ""}
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
    // 转成 React 节点，避免把不可信图片 src 插入真实 DOM；预览也不修改原始 steps。
    return [...fragment.childNodes].map((node, index) => render(node, String(index)));
  }, [bugId, value]);
  return <>
    <div className="bugs-steps">{content.length ? content : <p className="muted">暂无复现步骤</p>}</div>
    {preview && <ImageViewer src={preview} title="复现步骤图片预览" alt="BUG 复现步骤放大预览" onClose={() => setPreview("")} />}
  </>;
}
