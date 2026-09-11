import { useEffect, useMemo, useRef, useState } from "react";
import FullCalendar from "@fullcalendar/react";
import dayGridPlugin from "@fullcalendar/daygrid";
import timeGridPlugin from "@fullcalendar/timegrid";
import interactionPlugin from "@fullcalendar/interaction";
import listPlugin from "@fullcalendar/list";
import luxonPlugin from "@fullcalendar/luxon3";
import zhLocale from "@fullcalendar/core/locales/zh-cn";
import { ChevronLeft, ChevronRight, GripVertical } from "lucide-react";
import { DateTime } from "luxon";
import { listen } from "@tauri-apps/api/event";
import { native } from "../lib/api";
import Modal from "../components/Modal";
import { dingCalendarApi, type DingCalendarState, type DingCalendarEvent } from "../lib/ding-calendar";
import type { Task } from "../lib/types";
import { isTaskExpired } from "../lib/task-expiry";
import { useExpiryRefresh } from "../lib/use-expiry-refresh";
import { calendarAlmanac, hasHolidaySchedule } from "../lib/calendar-almanac";
import "./calendar.css";

interface Props {
  tasks: Task[];
  onEdit: (task: Task) => void;
  onCreate: (schedule: Task["schedule"]) => void;
  onChange: (task: Task, schedule: Task["schedule"]) => Promise<void>;
}

const views = [
  ["timeGridDay", "日"],
  ["timeGridWeek", "周"],
  ["dayGridMonth", "月"],
  ["listMonth", "列表"],
] as const;
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;

export default function CalendarView({
  tasks,
  onEdit,
  onCreate,
  onChange,
}: Props) {
  useExpiryRefresh();
  const calendar = useRef<FullCalendar>(null);
  const [view, setView] = useState("dayGridMonth");
  const [active, setActive] = useState<DateTime>(DateTime.now());
  const [month, setMonth] = useState<DateTime>(DateTime.now().startOf("month"));
  const [title, setTitle] = useState(DateTime.now().toFormat("yyyy年 M月"));
  const [sources, setSources] = useState({ local: true, zentao: true, dingtalk: true });
  const [error, setError] = useState("");
  const [ding, setDing] = useState<DingCalendarState | null>(null);
  const [detail, setDetail] = useState<DingCalendarEvent | null>(null);
  const dingRequest = useRef(0);
  useEffect(() => {
    if (!native) return;
    let stopped = false;
    let off: (() => void) | undefined;
    async function reload() {
      const request = ++dingRequest.current;
      try {
        const value = await dingCalendarApi.state();
        if (!stopped && request === dingRequest.current) setDing(value);
      } catch {
        // 后台读取失败保留当前日程；连接状态与错误统一在设置页展示。
      }
    }
    void reload();
    void listen("dingtalk-calendar-updated", () => { void reload(); })
      .then(unlisten => { if (stopped) unlisten(); else off = unlisten; })
      .catch(() => { /* 设置页负责展示连接状态；保留已读取日程。 */ });
    return () => { stopped = true; off?.(); };
  }, []);
  const dingEvents = sources.dingtalk && ding?.config.enabled
    ? ding.config.calendarUrl ? ding.events : [] : [];
  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      calendar.current?.getApi().updateSize();
      if (view.startsWith("timeGrid"))
        calendar.current?.getApi().scrollToTime("08:00:00");
    });
    return () => cancelAnimationFrame(frame);
  }, [view]);
  const visible = useMemo(
    () =>
      tasks.filter((task) => sources[task.source]),
    [tasks, sources],
  );
  const unscheduled = visible.filter((task) => !task.schedule);
  const start = month.startOf("week");
  const today = DateTime.now();
  const dayCount = Math.ceil((month.daysInMonth! + month.weekday - 1) / 7) * 7;
  const days = Array.from({ length: dayCount }, (_, index) =>
    start.plus({ days: index }),
  );
  function navigate(day: DateTime) {
    setActive(day);
    setMonth(day.startOf("month"));
    calendar.current?.getApi().gotoDate(day.toISODate()!);
  }
  function hasTask(day: DateTime) {
    return dingEvents.some(event => DateTime.fromISO(event.start, { zone }) < day.plus({ days: 1 }) && (event.end ? DateTime.fromISO(event.end, { zone }) : DateTime.fromISO(event.start, { zone }).plus(event.allDay ? { days: 1 } : { seconds: 1 })) > day) || visible.some((task) => {
      if (!task.schedule) return false;
      const startDay = DateTime.fromISO(task.schedule.start, { zone }).startOf(
        "day",
      );
      const end = task.schedule.end
        ? DateTime.fromISO(task.schedule.end, { zone })
        : startDay.plus({ days: 1 });
      return startDay < day.plus({ days: 1 }) && end > day;
    });
  }

  function almanac(day: DateTime, compact = false) {
    const info = calendarAlmanac(day.year, day.month, day.day);
    return <span className={compact ? "wb-mini-almanac" : "wb-day-almanac"} title={info.title}>
      {!compact && <span className="wb-lunar-label">{info.label}</span>}
      {info.kind && <span className={`wb-holiday-mark is-${info.kind}`} aria-label={info.title}>
        {info.kind === "work" ? "班" : "休"}
      </span>}
    </span>;
  }

  return (
    <div className="wb-calendar">
      <aside className="wb-calendar-sidebar" aria-label="日历导航与任务来源">
        <div className="wb-calendar-mini-heading">
          <strong>{month.toFormat("yyyy年 M月")}</strong>
          <div>
            <button
              className="wb-calendar-icon"
              aria-label="小日历上个月"
              title="上个月"
              onClick={() => setMonth(month.minus({ months: 1 }))}
            >
              <ChevronLeft size={16} />
            </button>
            <button
              className="wb-calendar-icon"
              aria-label="小日历下个月"
              title="下个月"
              onClick={() => setMonth(month.plus({ months: 1 }))}
            >
              <ChevronRight size={16} />
            </button>
          </div>
        </div>
        <div
          className="wb-calendar-mini"
          aria-label={month.toFormat("yyyy年 M月")}
        >
          {["一", "二", "三", "四", "五", "六", "日"].map((day) => (
            <span className="wb-calendar-weekday" key={day}>
              {day}
            </span>
          ))}
          {days.map((day) => (
            <button
              key={day.toISODate()}
              className={[
                "wb-calendar-mini-day",
                day.month !== month.month ? "is-outside" : "",
                day.hasSame(active, "day") ? "is-selected" : "",
                day.hasSame(today, "day") ? "is-today" : "",
              ].join(" ")}
              aria-label={day.toFormat("yyyy年M月d日")}
              aria-pressed={day.hasSame(active, "day")}
              aria-current={day.hasSame(today, "day") ? "date" : undefined}
              onClick={() => navigate(day)}
            >
              {day.day}
              {almanac(day, true)}
              {hasTask(day) && <i aria-hidden="true" />}
            </button>
          ))}
        </div>
        <section className="wb-calendar-sources">
          <h3>我的日历</h3>
          {(
            [
              ["local", "个人任务"],
              ["zentao", "禅道执行"],
              ["dingtalk", "钉钉日历"],
            ] as const
          ).map(([source, label]) => (
            <label key={source}>
              <input
                type="checkbox"
                checked={sources[source]}
                aria-label={label}
                onChange={(event) =>
                  setSources({ ...sources, [source]: event.target.checked })
                }
              />
              <span>{label}</span>
              <i
                className={`wb-calendar-source-dot ${source}`}
                aria-hidden="true"
              />
            </label>
          ))}
          <p className="wb-almanac-legend"><span className="wb-holiday-mark is-rest">休</span> 放假　<span className="wb-holiday-mark is-work">班</span> 调休上班</p>
          {!hasHolidaySchedule(month.year) && <p className="wb-almanac-legend">{month.year} 年放假调休数据尚未收录，仅显示农历与节气。</p>}
        </section>
        <section className="wb-calendar-unscheduled">
          <h3>
            未排期 <span>{unscheduled.length}</span>
          </h3>
          {unscheduled.length ? (
            <>
            {unscheduled.map((task) => (
              <button
                key={task.id}
                title={task.title}
                className={["done", "closed"].includes(task.status) ? "wb-calendar-unscheduled-done" : undefined}
                onClick={() => onEdit(task)}
              >
                <GripVertical size={13} />
                <span>{task.title}</span>
              </button>
            ))}
            </>
          ) : (
            <p>暂无未排期任务</p>
          )}
        </section>
      </aside>
      <div className="wb-calendar-main">
        <div className="wb-calendar-toolbar">
          <div className="wb-calendar-navigation">
            <button
              className="wb-calendar-today"
              onClick={() => navigate(DateTime.now())}
            >
              今天
            </button>
            <button
              className="wb-calendar-icon"
              aria-label="上一时段"
              title="上一时段"
              onClick={() => calendar.current?.getApi().prev()}
            >
              <ChevronLeft size={17} />
            </button>
            <button
              className="wb-calendar-icon"
              aria-label="下一时段"
              title="下一时段"
              onClick={() => calendar.current?.getApi().next()}
            >
              <ChevronRight size={17} />
            </button>
            <h2 aria-live="polite">{title}</h2>
          </div>
          <div
            className="wb-calendar-segmented"
            role="group"
            aria-label="日历视图"
          >
            {views.map(([key, label]) => (
              <button
                key={key}
                aria-pressed={view === key}
                className={view === key ? "is-active" : ""}
                onClick={() => calendar.current?.getApi().changeView(key)}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
        {error && (
          <p className="wb-calendar-error" role="alert">
            {error}
          </p>
        )}
        <FullCalendar
          ref={calendar}
          plugins={[
            dayGridPlugin,
            timeGridPlugin,
            interactionPlugin,
            listPlugin,
            luxonPlugin,
          ]}
          initialView="dayGridMonth"
          headerToolbar={false}
          locale={zhLocale}
          firstDay={1}
          timeZone={zone}
          height={view.startsWith("timeGrid") ? 720 : "auto"}
          fixedWeekCount={false}
          allDayText="全天"
          noEventsText="当前时段暂无任务或日程"
          moreLinkText={(count) => `+${count} 项`}
          dayMaxEvents={3}
          eventTimeFormat={{
            hour: "2-digit",
            minute: "2-digit",
            hour12: false,
          }}
          slotLabelFormat={{
            hour: "2-digit",
            minute: "2-digit",
            hour12: false,
          }}
          dayHeaderFormat={{ weekday: "short" }}
          dayCellContent={(info) => <span className="wb-calendar-day-heading">
            <span className="wb-day-number">{info.dayNumberText}</span>
            {almanac(DateTime.fromJSDate(info.date, { zone }))}
          </span>}
          dayHeaderContent={(info) => {
            const day = DateTime.fromJSDate(info.date, { zone });
            return info.view.type === "dayGridMonth"
              ? ["周一", "周二", "周三", "周四", "周五", "周六", "周日"][
                  day.weekday - 1
                ]
              : <span className="wb-calendar-day-heading">{`${["周一", "周二", "周三", "周四", "周五", "周六", "周日"][day.weekday - 1]} ${day.toFormat("M/d")}`}{almanac(day)}</span>;
          }}
          scrollTime="08:00:00"
          nowIndicator
          editable
          selectable
          selectMirror
          datesSet={(info) => {
            const date = DateTime.fromJSDate(info.view.calendar.getDate(), {
              zone,
            });
            setView(info.view.type);
            setActive(date);
            setMonth(date.startOf("month"));
            setTitle(
              info.view.type === "dayGridMonth" ||
                info.view.type === "listMonth"
                ? date.toFormat("yyyy年 M月")
                : info.view.title,
            );
          }}
          events={[...visible
            .filter((task) => task.schedule)
            .map((task) => ({
              id: task.id,
              title: task.title,
              start: task.schedule!.start,
              end: task.schedule!.end,
              allDay: task.schedule!.kind === "all_day",
              extendedProps: { expired: isTaskExpired(task) },
              classNames: [
                `wb-calendar-event-${["done", "closed"].includes(task.status) ? "done" : task.source}`,
              ],
            })), ...dingEvents.map(event => ({
              id: `dingtalk:${event.id}`,
              title: event.title,
              start: event.start,
              end: event.end,
              allDay: event.allDay,
              editable: false,
              startEditable: false,
              durationEditable: false,
              extendedProps: { dingtalk: true, eventId: event.id },
              classNames: ["wb-calendar-event-dingtalk"],
            }))]}
          eventContent={(info) => (
            <>
              {info.timeText && <span>{info.timeText} </span>}
              <span className="wb-calendar-event-title">{info.event.title}</span>
              {info.event.extendedProps.expired && (
                <span className="tag expired">已过期</span>
              )}
            </>
          )}
          eventClick={(info) => {
            if (info.event.extendedProps.dingtalk) {
              const event = dingEvents.find(event => event.id === info.event.extendedProps.eventId);
              if (event) setDetail(event);
              return;
            }
            const task = tasks.find((item) => item.id === info.event.id);
            if (task) onEdit(task);
          }}
          eventChange={async (info) => {
            const task = tasks.find((item) => item.id === info.event.id);
            if (!task) {
              info.revert();
              return;
            }
            setError("");
            try {
              await onChange(task, {
                kind: info.event.allDay ? "all_day" : "timed",
                timezone: zone,
                start: info.event.startStr,
                end: info.event.endStr || undefined,
              });
            } catch (reason) {
              info.revert();
              setError(
                reason instanceof Error ? reason.message : String(reason),
              );
            }
          }}
          select={(info) => {
            info.view.calendar.unselect();
            onCreate({
              kind: info.allDay ? "all_day" : "timed",
              timezone: zone,
              start: info.startStr,
              end: info.endStr,
            });
          }}
        />
      </div>
      {detail && <Modal title="钉钉日程详情" onClose={() => setDetail(null)}>
        <div className="wb-ding-detail">
          <span className="tag">钉钉 · 只读日程</span>
          <h3>{detail.title}</h3>
          <dl>
            <dt>开始{detail.allDay ? "日期" : "时间"}</dt><dd>{detail.start ? DateTime.fromISO(detail.start, { zone }).toFormat(detail.allDay ? "yyyy年M月d日" : "yyyy年M月d日 HH:mm") : "未排期"}{detail.allDay ? " · 全天" : ""}</dd>
            {detail.end && <><dt>结束{detail.allDay ? "日期" : "时间"}</dt><dd>{(detail.allDay ? DateTime.fromISO(detail.end, { zone }).minus({ days: 1 }) : DateTime.fromISO(detail.end, { zone })).toFormat(detail.allDay ? "yyyy年M月d日" : "yyyy年M月d日 HH:mm")}</dd></>}
            <dt>所属日历</dt><dd>{ding?.config.calendarName || "钉钉日历"}</dd>
            {detail.location && <><dt>地点</dt><dd>{detail.location}</dd></>}
            {detail.description && <><dt>说明</dt><dd className="wb-ding-description">{detail.description}</dd></>}
          </dl>
          <p className="muted">如需修改日程，请在钉钉中操作，再同步到栖点。</p>
        </div>
      </Modal>}
    </div>
  );
}
