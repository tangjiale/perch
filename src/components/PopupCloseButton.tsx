import type { ComponentPropsWithRef } from "react";
import { X } from "lucide-react";
import "./popup-close-button.css";

type PopupCloseButtonProps = Pick<ComponentPropsWithRef<"button">, "disabled" | "onClick" | "ref"> & {
  label?: string;
};

export default function PopupCloseButton({ label = "关闭", ...props }: PopupCloseButtonProps) {
  return (
    <button {...props} className="popup-close" type="button" title={label} aria-label={label}>
      <X size={16} strokeWidth={1.8} aria-hidden="true" />
    </button>
  );
}
