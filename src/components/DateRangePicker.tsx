import * as Popover from "@radix-ui/react-popover";
import { CalendarDays, ChevronLeft, ChevronRight } from "lucide-react";
import { DateTime } from "luxon";
import { useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import "./date-range-picker.css";

export interface DateRange {
  start: string;
  end: string;
}

const weekdays = ["一", "二", "三", "四", "五", "六", "日"];

export default function DateRangePicker({
  id,
  value,
  onChange,
}: {
  id: string;
  value: DateRange | null;
  onChange: (range: DateRange | null) => void;
}) {
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState<DateTime>(() => DateTime.local().startOf("day"));
  const [start, setStart] = useState<string | null>(null);
  const dayButton = useRef<HTMLButtonElement>(null);
  const focusDay = useRef(false);
  const month = cursor.startOf("month");
  const monthLabel = month.toFormat("yyyy年M月");
  const today = DateTime.local().toFormat("yyyy-MM-dd");
  const range = start ? { start, end: start } : value;

  useLayoutEffect(() => {
    if (!focusDay.current) return;
    focusDay.current = false;
    dayButton.current?.focus();
  }, [cursor]);

  function openCalendar(next: boolean) {
    if (next) {
      setStart(null);
      setCursor(value ? DateTime.fromISO(value.start) : DateTime.local().startOf("day"));
    }
    setOpen(next);
  }

  function selectDate(day: DateTime) {
    const date = day.toFormat("yyyy-MM-dd");
    if (!start) {
      focusDay.current = true;
      setCursor(day);
      setStart(date);
      return;
    }
    onChange(date < start ? { start: date, end: start } : { start, end: date });
    setOpen(false);
  }

  function moveFocus(event: KeyboardEvent<HTMLButtonElement>, day: DateTime) {
    let next: DateTime;
    switch (event.key) {
      case "ArrowLeft": next = day.minus({ days: 1 }); break;
      case "ArrowRight": next = day.plus({ days: 1 }); break;
      case "ArrowUp": next = day.minus({ days: 7 }); break;
      case "ArrowDown": next = day.plus({ days: 7 }); break;
      case "PageUp": next = day.minus({ months: 1 }); break;
      case "PageDown": next = day.plus({ months: 1 }); break;
      default: return;
    }
    event.preventDefault();
    focusDay.current = true;
    setCursor(next);
  }

  return (
    <Popover.Root open={open} onOpenChange={openCalendar}>
      <Popover.Trigger id={id} className="glass-select-trigger date-range-trigger" aria-describedby={`${id}-value`}>
        <span id={`${id}-value`} className="date-range-value">
          {value ? (
            <>
              <time dateTime={value.start}>{value.start}</time>
              <span>至</span>
              <time dateTime={value.end}>{value.end}</time>
            </>
          ) : "选择日期范围"}
        </span>
        <CalendarDays size={15} className="glass-select-leading" aria-hidden="true" />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          className="glass-select-content date-range-popover"
          aria-label="选择日期范围"
          align="start"
          sideOffset={7}
          collisionPadding={12}
          onOpenAutoFocus={(event) => {
            if (!dayButton.current) return;
            event.preventDefault();
            dayButton.current.focus();
          }}
        >
          <div className="date-range-month">
            <button type="button" className="icon-button" aria-label="上个月" onClick={() => setCursor(cursor.minus({ months: 1 }))}>
              <ChevronLeft size={16} aria-hidden="true" />
            </button>
            <strong aria-live="polite">{monthLabel}</strong>
            <button type="button" className="icon-button" aria-label="下个月" onClick={() => setCursor(cursor.plus({ months: 1 }))}>
              <ChevronRight size={16} aria-hidden="true" />
            </button>
          </div>
          <div className="date-range-days" role="group" aria-label={monthLabel}>
            {weekdays.map((day) => <span key={day} className="date-range-weekday" aria-hidden="true">{day}</span>)}
            {Array.from({ length: month.daysInMonth ?? 0 }, (_, index) => {
              const day = month.plus({ days: index });
              const date = day.toFormat("yyyy-MM-dd");
              const active = day.day === cursor.day;
              const selected = Boolean(range && date >= range.start && date <= range.end);
              const endpoint = date === range?.start || date === range?.end;
              return (
                <button
                  key={date}
                  ref={active ? dayButton : undefined}
                  type="button"
                  className="date-range-day"
                  style={index === 0 ? { gridColumnStart: month.weekday } : undefined}
                  aria-label={date}
                  aria-current={date === today ? "date" : undefined}
                  aria-pressed={selected}
                  data-endpoint={endpoint || undefined}
                  tabIndex={active ? 0 : -1}
                  onClick={() => selectDate(day)}
                  onKeyDown={(event) => moveFocus(event, day)}
                >
                  {day.day}
                </button>
              );
            })}
          </div>
          <div className="date-range-footer">
            <span role="status">{start ? "请选择结束日期" : "请选择开始日期"}</span>
            <button type="button" className="text-link" disabled={!value && !start} onClick={() => { onChange(null); setOpen(false); }}>
              清除
            </button>
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
