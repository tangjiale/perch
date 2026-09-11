import { useEffect, useMemo, useState } from "react";
import { Solar } from "lunar-typescript";

export default function TopbarClock() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    // 每次读取系统时间，避免计数累积误差；恢复窗口时立即校准。
    const update = () => setNow(new Date());
    const timer = window.setInterval(update, 1000);
    window.addEventListener("focus", update);
    document.addEventListener("visibilitychange", update);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", update);
      document.removeEventListener("visibilitychange", update);
    };
  }, []);
  const year = now.getFullYear(), month = now.getMonth() + 1, day = now.getDate();
  const lunar = useMemo(() => {
    const date = Solar.fromYmd(year, month, day).getLunar();
    return `农历${date.getMonthInChinese()}月${date.getDayInChinese()}`;
  }, [year, month, day]);
  const weekday = ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"][now.getDay()];
  const time = [now.getHours(), now.getMinutes(), now.getSeconds()]
    .map((value) => String(value).padStart(2, "0")).join(":");
  return (
    <div className="topbar-clock" aria-label="当前日期和时间">
      <span className="topbar-clock-date">{year}年{month}月{day}日</span>
      <span className="topbar-clock-weekday">{weekday}</span>
      <span className="topbar-clock-divider" aria-hidden="true" />
      <time dateTime={now.toISOString()}>{time}</time>
      <span className="topbar-clock-divider" aria-hidden="true" />
      <span className="topbar-clock-lunar">{lunar}</span>
    </div>
  );
}
