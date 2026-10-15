import { useEffect, useRef, useState, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import * as Popover from "@radix-ui/react-popover";
import { Copy, Download } from "lucide-react";
import { api } from "../lib/api";
import "./image-actions.css";

type Menu = { dataUrl: string; container: HTMLElement };
type Feedback = { text: string; error?: boolean; container: HTMLElement };

/** 正文和大图共用中文菜单；图片操作不经过编辑器的正文更新。 */
export default function useImageActions() {
  const [menu, setMenu] = useState<Menu | null>(null);
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const anchor = useRef({ getBoundingClientRect: () => new DOMRect() });
  const focusTarget = useRef<HTMLImageElement | null>(null);
  const inFlight = useRef(false);
  useEffect(() => {
    if (!feedback || inFlight.current) return;
    const timer = setTimeout(() => setFeedback(null), feedback.error ? 6000 : 2500);
    return () => clearTimeout(timer);
  }, [feedback]);

  function onContextMenu(event: MouseEvent<HTMLImageElement>) {
    event.preventDefault();
    event.stopPropagation();
    const image = event.currentTarget;
    const dataUrl = image.currentSrc || image.src;
    if (inFlight.current || !image.complete || !image.naturalWidth || !/^data:image\/(png|jpeg|gif|webp);base64,/i.test(dataUrl)) return;
    const rect = image.getBoundingClientRect();
    const x = event.clientX || rect.left + Math.min(rect.width / 2, 32);
    const y = event.clientY || rect.top + Math.min(rect.height / 2, 32);
    anchor.current = { getBoundingClientRect: () => new DOMRect(x, y, 0, 0) };
    focusTarget.current = image;
    setFeedback(null);
    // dialog 位于 top layer，菜单须挂到当前弹窗内才能显示和响应点击。
    setMenu({ dataUrl, container: image.closest("dialog") ?? document.body });
  }

  async function act(action: "copy" | "download") {
    if (!menu || inFlight.current) return;
    const { dataUrl, container } = menu;
    inFlight.current = true;
    setMenu(null);
    setFeedback({ text: action === "copy" ? "正在复制图片…" : "请选择图片保存位置…", container });
    try {
      if (action === "copy") {
        await api.copyImage(dataUrl);
        setFeedback({ text: "图片已复制", container });
      } else {
        const saved = await api.downloadImage(dataUrl);
        setFeedback(saved ? { text: "图片已保存", container } : null);
      }
    } catch (cause) {
      const detail = cause instanceof Error ? cause.message : "请重试";
      setFeedback({ text: `${action === "copy" ? "复制" : "下载"}失败：${detail}`, error: true, container });
    } finally {
      inFlight.current = false;
    }
  }

  const overlay = <>
    <Popover.Root open={!!menu} onOpenChange={open => { if (!open) setMenu(null); }}>
      <Popover.Anchor virtualRef={anchor} />
      <Popover.Portal container={menu?.container}>
        <Popover.Content className="image-actions-menu" role="menu" aria-label="图片操作" side="right" align="start" sideOffset={2} collisionPadding={8}
          onContextMenu={event => event.preventDefault()} onClick={event => event.stopPropagation()}
          onCloseAutoFocus={event => { event.preventDefault(); focusTarget.current?.focus({ preventScroll: true }); }}
          onEscapeKeyDown={event => { event.preventDefault(); event.stopPropagation(); setMenu(null); }}
          onKeyDown={event => {
            if (event.key === "Tab") { setMenu(null); return; }
            const items = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
            const index = items.indexOf(document.activeElement as HTMLButtonElement);
            const next = event.key === "ArrowDown" ? (index + 1) % items.length : event.key === "ArrowUp" ? (index + items.length - 1) % items.length : event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : -1;
            if (next >= 0) { event.preventDefault(); event.stopPropagation(); items[next].focus(); }
          }}>
          <button type="button" role="menuitem" onClick={() => void act("copy")}><Copy size={16} />复制图片</button>
          <button type="button" role="menuitem" onClick={() => void act("download")}><Download size={16} />下载图片</button>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
    {feedback && createPortal(<div className={`image-actions-feedback${feedback.error ? " is-error" : ""}`} role={feedback.error ? "alert" : "status"}>{feedback.text}</div>, feedback.container)}
  </>;
  return { onContextMenu, overlay };
}
