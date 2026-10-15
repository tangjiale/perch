import { useEffect, useId, useRef, useState } from "react";
import * as Popover from "@radix-ui/react-popover";
import { Check, ChevronDown, Search } from "lucide-react";
import type { GlassSelectProps } from "./GlassSelect";
import "./glass-search-select.css";

export default function GlassSearchSelect({ options, value, onValueChange, id, name, required, disabled,
  className = "", title, placeholder = "请选择", icon, searchPlaceholder = "搜索选项", ...aria
}: Omit<GlassSelectProps, "searchable">) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [container, setContainer] = useState<HTMLElement>();
  const trigger = useRef<HTMLButtonElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const listId = useId();
  const selected = options.find(option => option.value === value);
  const needle = query.trim().toLocaleLowerCase();
  const filtered = options.filter(option => `${option.label} ${option.value}`.toLocaleLowerCase().includes(needle));
  const enabled = filtered.filter(option => !option.disabled);
  const activeValue = enabled[Math.min(active, enabled.length - 1)]?.value;
  const activeIndex = filtered.findIndex(option => option.value === activeValue);
  useEffect(() => {
    list.current?.querySelector('[data-highlighted="true"]')?.scrollIntoView({ block: "nearest" });
  }, [activeValue, open]);
  useEffect(() => { if (disabled) setOpen(false); }, [disabled]);
  function choose(next: string) { onValueChange(next); setOpen(false); }
  return <Popover.Root open={open} onOpenChange={next => {
    if (disabled) return;
    if (next) {
      setContainer(trigger.current?.closest("dialog") ?? undefined);
      setQuery("");
      setActive(Math.max(0, options.filter(option => !option.disabled).findIndex(option => option.value === value)));
    }
    setOpen(next);
  }}>
    {name && <input type="hidden" name={name} value={value} />}
    <Popover.Trigger asChild>
      <button {...aria} type="button" ref={trigger} id={id} disabled={disabled} role="combobox" aria-haspopup="listbox" aria-expanded={open} aria-controls={open ? listId : undefined} aria-required={required}
        title={title ?? selected?.label} className={`glass-select-trigger ${className}`}>
        {icon && <span className="glass-select-leading">{icon}</span>}
        <span className="glass-select-value">{selected?.label || placeholder}</span>
        <ChevronDown className="glass-select-chevron" size={15} />
      </button>
    </Popover.Trigger>
    <Popover.Portal container={container}>
      <Popover.Content className="glass-select-content glass-search-content" sideOffset={7} collisionPadding={12} align="start" aria-label={aria["aria-label"] ?? "选择选项"}
        onOpenAutoFocus={event => { event.preventDefault(); input.current?.focus(); }}
        onEscapeKeyDown={event => { event.preventDefault(); event.stopPropagation(); setOpen(false); }}>
        <div className="glass-search-input">
          <Search size={15} aria-hidden="true" />
          <input ref={input} role="combobox" aria-label={searchPlaceholder} aria-autocomplete="list" aria-expanded="true" aria-controls={listId}
            aria-activedescendant={activeIndex >= 0 ? `${listId}-${activeIndex}` : undefined}
            placeholder={searchPlaceholder} value={query} onChange={event => { setQuery(event.target.value); setActive(0); }}
            onKeyDown={event => {
              if (event.nativeEvent.isComposing || event.keyCode === 229) return;
              if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                event.preventDefault();
                if (enabled.length) setActive(index => (index + (event.key === "ArrowDown" ? 1 : -1) + enabled.length) % enabled.length);
              } else if (event.key === "Enter") {
                event.preventDefault();
                if (activeValue !== undefined) choose(activeValue);
              }
            }} />
        </div>
        <div ref={list} id={listId} role="listbox" aria-label={aria["aria-label"] ?? "选项"} className="glass-search-options">
          {filtered.map((option, index) => <div key={option.value} id={`${listId}-${index}`} role="option" aria-selected={option.value === value} aria-disabled={option.disabled || undefined}
            className="glass-select-item" data-selected={option.value === value || undefined} data-disabled={option.disabled || undefined} data-highlighted={option.value === activeValue || undefined}
            title={option.label} onPointerMove={() => { if (!option.disabled) setActive(enabled.findIndex(item => item.value === option.value)); }}
            onMouseDown={event => event.preventDefault()} onClick={() => { if (!option.disabled) choose(option.value); }}>
            <span className="glass-select-check">{option.value === value && <Check size={15} />}</span><span>{option.label}</span>
          </div>)}
          {!filtered.length && <div className="glass-select-empty" role="status">{query ? "没有匹配的选项" : "暂无可选项"}</div>}
        </div>
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>;
}
