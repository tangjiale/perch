import { useEffect, useId, useRef, type ReactNode } from "react";
import PopupCloseButton from "./PopupCloseButton";
export default function Modal({
  title,
  children,
  onClose,
  wide = false,
  busy = false,
  className = "",
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  wide?: boolean;
  busy?: boolean;
  className?: string;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const titleRef = useRef<HTMLHeadingElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    dialog.showModal();
    // 浏览器默认选中关闭按钮时，先把焦点放在标题上，Tab 仍可到达关闭按钮。
    if (document.activeElement === closeRef.current) {
      titleRef.current?.focus({ preventScroll: true });
    }
    return () => dialog.close();
  }, []);
  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={titleId}
      className={`${wide ? "modal wide" : "modal"} ${className}`.trim()}
      onKeyDown={(event) => {
        // 原生 cancel 事件可能不可取消，先阻止 Esc 的默认关闭行为。
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
        }
      }}
      onCancel={(event) => {
        // 禁止 Esc 隐式关闭，避免误操作丢失未保存的表单内容。
        event.preventDefault();
        event.stopPropagation();
      }}
    >
      <header>
        <h2 id={titleId} ref={titleRef} tabIndex={-1}>{title}</h2>
        <PopupCloseButton ref={closeRef} disabled={busy} onClick={onClose} />
      </header>
      {children}
    </dialog>
  );
}
