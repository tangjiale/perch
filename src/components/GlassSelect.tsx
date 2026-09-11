import { useRef, useState, type AriaAttributes, type ReactNode } from "react";
import * as Select from "@radix-ui/react-select";
import { Check, ChevronDown, ChevronUp } from "lucide-react";
import "./glass-select.css";

export interface GlassSelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

export interface GlassSelectProps extends AriaAttributes {
  options: GlassSelectOption[];
  value: string;
  onValueChange: (value: string) => void;
  id?: string;
  name?: string;
  required?: boolean;
  disabled?: boolean;
  className?: string;
  title?: string;
  placeholder?: string;
  icon?: ReactNode;
}

export default function GlassSelect({
  options,
  value,
  onValueChange,
  id,
  name,
  required,
  disabled,
  className = "",
  title,
  placeholder = "请选择",
  icon,
  ...aria
}: GlassSelectProps) {
  const trigger = useRef<HTMLButtonElement>(null);
  const [container, setContainer] = useState<HTMLElement | undefined>();
  const [open, setOpen] = useState(false);
  const selected = options.find((option) => option.value === value);
  let emptyValue = "__glass_select_empty__";
  while (options.some((option) => option.value === emptyValue))
    emptyValue += "_";

  return (
    <Select.Root
      value={value}
      onValueChange={(next) => onValueChange(next === emptyValue ? "" : next)}
      open={open}
      onOpenChange={(next) => {
        // 原生 dialog 位于 top layer，菜单必须与触发器处于同一弹窗中。
        if (next) setContainer(trigger.current?.closest("dialog") ?? undefined);
        setOpen(next);
      }}
      name={name}
      required={required}
      disabled={disabled}
    >
      <Select.Trigger
        {...aria}
        ref={trigger}
        id={id}
        className={`glass-select-trigger ${className}`}
        title={title ?? selected?.label}
      >
        {icon && (
          <span className="glass-select-leading" aria-hidden="true">
            {icon}
          </span>
        )}
        <span className="glass-select-value">
          <Select.Value placeholder={selected?.label ?? placeholder}>
            {selected?.label ?? placeholder}
          </Select.Value>
        </span>
        <Select.Icon className="glass-select-chevron">
          <ChevronDown size={15} />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal container={container}>
        <Select.Content
          className="glass-select-content"
          position="popper"
          sideOffset={7}
          collisionPadding={12}
          align="start"
          onEscapeKeyDown={(event) => event.stopPropagation()}
        >
          <Select.ScrollUpButton className="glass-select-scroll">
            <ChevronUp size={15} />
          </Select.ScrollUpButton>
          <Select.Viewport className="glass-select-viewport">
            {options.length ? (
              options.map((option) => (
                <Select.Item
                  key={option.value}
                  value={option.value || emptyValue}
                  disabled={option.disabled}
                  textValue={option.label}
                  title={option.label}
                  className="glass-select-item"
                  data-selected={option.value === value || undefined}
                  aria-selected={option.value === value}
                >
                  <span className="glass-select-check" aria-hidden="true">
                    {option.value === value && (
                      <Check size={15} strokeWidth={2.2} />
                    )}
                  </span>
                  <Select.ItemText>{option.label}</Select.ItemText>
                </Select.Item>
              ))
            ) : (
              <div className="glass-select-empty">暂无可选项</div>
            )}
          </Select.Viewport>
          <Select.ScrollDownButton className="glass-select-scroll">
            <ChevronDown size={15} />
          </Select.ScrollDownButton>
        </Select.Content>
      </Select.Portal>
    </Select.Root>
  );
}
