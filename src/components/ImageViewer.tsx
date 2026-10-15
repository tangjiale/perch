import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { createPortal } from "react-dom";
import { Maximize, ZoomIn, ZoomOut } from "lucide-react";
import Modal from "./Modal";
import useImageActions from "./useImageActions";
import "./image-viewer.css";

type Size = { width: number; height: number };
type View = { scale: number; x: number; y: number; fitted: boolean };
const clamp = (value: number, limit: number) => Math.max(-limit, Math.min(limit, value));

/** 独立于表单的看图画布；缩放和平移只影响预览，不改变原始图片引用。 */
export default function ImageViewer({ src, title = "图片预览", alt = "图片放大预览", onClose }: {
  src: string; title?: string; alt?: string; onClose: () => void;
}) {
  const stage = useRef<HTMLDivElement>(null);
  const imageActions = useImageActions();
  const hintId = useId();
  const [natural, setNatural] = useState<Size>({ width: 0, height: 0 });
  const [viewport, setViewport] = useState<Size>({ width: 0, height: 0 });
  const [view, setView] = useState<View>({ scale: 1, x: 0, y: 0, fitted: true });
  const [dragging, setDragging] = useState(false);
  const [failed, setFailed] = useState(false);
  const drag = useRef<{ pointerId: number; startX: number; startY: number; x: number; y: number; moved: boolean; image: boolean } | null>(null);
  const ready = natural.width > 0 && viewport.width > 0 && viewport.height > 0;
  const fitScale = ready ? Math.min(1, viewport.width / natural.width, viewport.height / natural.height) : 1;
  const minScale = Math.min(0.1, fitScale);
  const bounded = useCallback((next: View): View => ({
    ...next,
    x: clamp(next.x, Math.max(0, (natural.width * next.scale - viewport.width) / 2)),
    y: clamp(next.y, Math.max(0, (natural.height * next.scale - viewport.height) / 2)),
  }), [natural, viewport]);

  useEffect(() => {
    const element = stage.current;
    if (!element) return;
    const measure = () => setViewport({ width: element.clientWidth, height: element.clientHeight });
    // Modal 的 showModal 生效后立即测量，不依赖首次 ResizeObserver 通知。
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  useLayoutEffect(() => {
    setView(previous => previous.fitted ? { scale: fitScale, x: 0, y: 0, fitted: true } : bounded(previous));
  }, [fitScale, bounded]);

  const fit = () => setView({ scale: fitScale, x: 0, y: 0, fitted: true });
  const zoom = useCallback((scale: number | ((current: number) => number), anchor = { x: 0, y: 0 }) => {
    setView(previous => {
      const requested = typeof scale === "function" ? scale(previous.scale) : scale;
      const nextScale = Math.max(minScale, Math.min(8, requested));
      const ratio = nextScale / previous.scale;
      // 以鼠标所在图像位置为中心缩放，避免细节在滚轮操作时跳走。
      return bounded({ scale: nextScale, x: anchor.x - (anchor.x - previous.x) * ratio, y: anchor.y - (anchor.y - previous.y) * ratio, fitted: false });
    });
  }, [bounded, minScale]);
  const anchorAt = (clientX: number, clientY: number) => {
    const rect = stage.current!.getBoundingClientRect();
    return { x: clientX - rect.left - rect.width / 2, y: clientY - rect.top - rect.height / 2 };
  };
  const toggleZoom = (anchor = { x: 0, y: 0 }) => {
    if (view.scale > fitScale + 0.001) fit();
    else zoom(Math.max(1, fitScale * 2), anchor);
  };
  useEffect(() => {
    const element = stage.current;
    if (!element || !ready) return;
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? element.clientHeight : 1;
      const delta = Math.max(-500, Math.min(500, event.deltaY * unit));
      const rect = element.getBoundingClientRect();
      zoom(scale => scale * Math.exp(-delta * 0.002), { x: event.clientX - rect.left - rect.width / 2, y: event.clientY - rect.top - rect.height / 2 });
    };
    // WebView 的默认 wheel 监听可能为 passive，需明确阻止页面跟随滚动。
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, [ready, zoom]);

  function pointerDown(event: PointerEvent<HTMLDivElement>) {
    if (!ready || event.button !== 0 || !event.isPrimary) return;
    event.preventDefault();
    if (event.target instanceof HTMLImageElement) event.target.focus({ preventScroll: true });
    drag.current = { pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, x: view.x, y: view.y, moved: false, image: event.target instanceof HTMLImageElement };
    event.currentTarget.setPointerCapture(event.pointerId);
  }
  function pointerMove(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    const dx = event.clientX - current.startX;
    const dy = event.clientY - current.startY;
    if (!current.moved && Math.hypot(dx, dy) < 4) return;
    current.moved = true;
    setDragging(true);
    setView(previous => bounded({ ...previous, x: current.x + dx, y: current.y + dy }));
  }
  function pointerUp(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    drag.current = null;
    setDragging(false);
    event.currentTarget.releasePointerCapture(event.pointerId);
    if (!current.moved && current.image) toggleZoom(anchorAt(event.clientX, event.clientY));
  }
  function keyboard(event: KeyboardEvent) {
    if (!ready) return;
    if (event.key === "+" || event.key === "=") zoom(scale => scale * 1.25);
    else if (event.key === "-" || event.key === "_") zoom(scale => scale / 1.25);
    else if (event.key === "0") fit();
    else if (event.key === "1") zoom(1);
    else return;
    event.preventDefault();
    event.stopPropagation();
  }
  return createPortal(<div onClick={event => event.stopPropagation()} onKeyDown={keyboard}>
    <Modal title={title} wide className="image-viewer" onClose={onClose}>
      <div className="image-viewer-toolbar" role="toolbar" aria-label="图片缩放">
        <button type="button" aria-label="缩小图片" title="缩小（−）" disabled={!ready || view.scale <= minScale} onClick={() => zoom(scale => scale / 1.25)}><ZoomOut size={18} /></button>
        <output aria-label="缩放比例">{Math.round(view.scale * 100)}%</output>
        <button type="button" aria-label="放大图片" title="放大（+）" disabled={!ready || view.scale >= 8} onClick={() => zoom(scale => scale * 1.25)}><ZoomIn size={18} /></button>
        <span className="image-viewer-toolbar-divider" />
        <button type="button" disabled={!ready} onClick={() => zoom(1)}>原始尺寸</button>
        <button type="button" disabled={!ready} onClick={fit}><Maximize size={16} />适应窗口</button>
      </div>
      <div ref={stage} className={`image-viewer-stage${dragging ? " is-dragging" : ""}`} data-scale={view.scale} data-x={view.x} data-y={view.y}
        onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp}
        onPointerCancel={() => { drag.current = null; setDragging(false); }}
        onLostPointerCapture={() => { drag.current = null; setDragging(false); }}>
        {failed && <p role="alert">图片无法显示，请关闭预览后重新加载。</p>}
        <img className="image-viewer-image" onContextMenu={imageActions.onContextMenu} src={src} alt={alt} draggable={false} role="button" tabIndex={0}
          aria-label={view.scale > fitScale + 0.001 ? "点击图片还原" : "点击图片放大"} aria-describedby={hintId}
          style={{ width: natural.width * view.scale || undefined, height: natural.height * view.scale || undefined, left: `calc(50% + ${view.x}px)`, top: `calc(50% + ${view.y}px)`, visibility: ready && !failed ? "visible" : "hidden", cursor: view.scale > fitScale + 0.001 ? "zoom-out" : "zoom-in" }}
          onLoad={event => { setFailed(false); setNatural({ width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight }); }} onError={() => setFailed(true)}
          onClick={event => { if (event.detail === 0) toggleZoom(); }}
          onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); event.stopPropagation(); toggleZoom(); } }} />
      </div>
      <p className="image-viewer-hint" id={hintId}>点击放大或还原 · 滚轮缩放 · 拖动查看 · 右键复制或下载 · 右上角关闭</p>
      {imageActions.overlay}
    </Modal>
  </div>, document.body);
}
