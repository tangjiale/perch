import { useEffect, useRef, useState } from "react";
import { init, use, type EChartsCoreOption } from "echarts/core";
import { BarChart, LineChart, PieChart, HeatmapChart } from "echarts/charts";
import {
  GridComponent,
  TooltipComponent,
  LegendComponent,
  CalendarComponent,
  VisualMapComponent,
  DataZoomComponent,
  AriaComponent,
} from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import { ChartNoAxesCombined, Table2 } from "lucide-react";

use([
  BarChart,
  LineChart,
  PieChart,
  HeatmapChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
  CalendarComponent,
  VisualMapComponent,
  DataZoomComponent,
  AriaComponent,
  CanvasRenderer,
]);

export interface ChartPalette {
  text: string;
  muted: string;
  line: string;
  surface: string;
  colors: string[];
}
function palette(): ChartPalette {
  const style = getComputedStyle(document.documentElement);
  const dark = document.documentElement.dataset.theme === "dark";
  return {
    text: style.getPropertyValue("--text").trim() || "#27272d",
    muted: style.getPropertyValue("--muted").trim(),
    line: style.getPropertyValue("--line").trim(),
    surface: dark ? "#303236" : "#ffffff",
    colors: dark
      ? [
          "#91a5b7",
          "#85b4f5",
          "#83cbae",
          "#b6acbe",
          "#e4b477",
          "#dd959d",
          "#75c7ce",
        ]
      : [
          "#75899c",
          "#397bc9",
          "#359579",
          "#92849c",
          "#bd843e",
          "#b85d71",
          "#29949c",
        ],
  };
}

export default function StatisticsChart({
  title,
  subtitle,
  option,
  rows,
  columns,
  empty,
  wide = false,
}: {
  title: string;
  subtitle?: string;
  option: (theme: ChartPalette) => EChartsCoreOption;
  rows: (string | number)[][];
  columns: string[];
  empty?: string;
  wide?: boolean;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [table, setTable] = useState(false);
  useEffect(() => {
    if (!host.current || table || empty) return;
    const chart = init(host.current, undefined, { renderer: "canvas" });
    const media = matchMedia("(prefers-reduced-motion: reduce)");
    const render = () => {
      const theme = palette();
      chart.setOption(
        {
          color: theme.colors,
          backgroundColor: "transparent",
          animation: !media.matches,
          animationDuration: 350,
          textStyle: {
            fontFamily:
              '-apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", sans-serif',
            color: theme.text,
          },
          aria: { enabled: true, label: { description: title } },
          tooltip: {
            trigger: "item",
            renderMode: "richText",
            confine: true,
            backgroundColor: theme.surface,
            borderColor: theme.line,
            textStyle: { color: theme.text },
          },
          ...option(theme),
        },
        { notMerge: true },
      );
    };
    render();
    let frame = 0;
    const resize = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => chart.resize());
    });
    resize.observe(host.current);
    const appearance = new MutationObserver(render);
    appearance.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme", "style"],
    });
    media.addEventListener("change", render);
    return () => {
      resize.disconnect();
      appearance.disconnect();
      cancelAnimationFrame(frame);
      media.removeEventListener("change", render);
      chart.dispose();
    };
  }, [option, title, table, empty]);

  return (
    <section
      className={`statistics-panel${wide ? " statistics-wide" : ""}`}
      aria-label={title}
    >
      <header className="statistics-panel-heading">
        <div>
          <h2>{title}</h2>
          {subtitle && <p>{subtitle}</p>}
        </div>
        <button
          type="button"
          className="icon-button"
          title={table ? "显示图表" : "查看数据表"}
          aria-label={`${title}：${table ? "显示图表" : "查看数据表"}`}
          aria-pressed={table}
          disabled={!!empty}
          onClick={() => setTable(!table)}
        >
          {table ? <ChartNoAxesCombined size={16} /> : <Table2 size={16} />}
        </button>
      </header>
      {empty ? (
        <div key="empty" className="statistics-chart-empty">
          <ChartNoAxesCombined size={28} />
          <span>{empty}</span>
        </div>
      ) : table ? (
        <div key="table" className="statistics-data-table">
          <table>
            <caption className="sr-only">{title}</caption>
            <thead>
              <tr>
                {columns.map((name) => (
                  <th key={name} scope="col">
                    {name}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row, index) => (
                <tr key={index}>
                  {row.map((cell, col) => (
                    <td key={col}>{cell}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div
          key="chart"
          className="statistics-canvas"
          role="img"
          aria-label={title}
          ref={host}
        />
      )}
    </section>
  );
}
