import { useEffect, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { RefreshCw, FolderKanban, LibraryBig, Bot, Bug } from "lucide-react";
import type { EChartsCoreOption } from "echarts/core";
import StatisticsChart, {
  type ChartPalette,
} from "../components/StatisticsChart";
import GlassSelect from "../components/GlassSelect";
import { buildStatistics, type Count } from "../lib/statistics";
import { buildBugStatistics } from "../lib/bug-statistics";
import type { PageProps } from "../lib/types";
import { command, native } from "../lib/api";
import "./statistics.css";

type Option = (theme: ChartPalette) => EChartsCoreOption;
interface McpSummary {
  id: string;
  name: string;
  enabled: boolean;
  transport: string;
}
const tabs = [
  ["work", "工作与项目", FolderKanban],
  ["bugs", "BUG 修复", Bug],
  ["resources", "知识与应用", LibraryBig],
  ["ai", "AI 与连接", Bot],
] as const;
const number = (value: number) => value.toLocaleString("zh-CN");
const percent = (value: number | null) =>
  value === null ? "暂无" : `${(value * 100).toFixed(1)}%`;
function bytes(value: number) {
  if (value < 1024) return `${number(value)} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / 1024 ** 2).toFixed(1)} MiB`;
}
const countRows = (counts: Count[]) =>
  counts.map((item) => [item.name, item.value]);

function donut(data: Count[], colorIndices?: number[]): Option {
  return (theme) => ({
    legend: {
      bottom: 0,
      type: "scroll",
      itemWidth: 9,
      itemHeight: 9,
      textStyle: { color: theme.muted, fontSize: 11 },
    },
    series: [
      {
        type: "pie",
        radius: ["48%", "70%"],
        center: ["50%", "44%"],
        avoidLabelOverlap: true,
        label: { show: false },
        labelLine: { show: false },
        emphasis: { scaleSize: 5, label: { show: false } },
        itemStyle: { borderColor: theme.surface, borderWidth: 3 },
        data: data.map((item, index) => ({
          ...item,
          itemStyle: {
            color:
              theme.colors[
                colorIndices?.[index] ?? index % theme.colors.length
              ],
          },
        })),
      },
    ],
  });
}
function bars(data: Count[], seriesName: string): Option {
  return (theme) => ({
    grid: { left: 110, right: data.length > 7 ? 34 : 30, top: 8, bottom: 28 },
    xAxis: {
      type: "value",
      minInterval: 1,
      axisLabel: { color: theme.muted, fontSize: 10 },
      splitLine: { lineStyle: { color: theme.line } },
    },
    yAxis: {
      type: "category",
      inverse: true,
      data: data.map((item) => item.name),
      axisTick: { show: false },
      axisLine: { show: false },
      axisLabel: {
        color: theme.muted,
        width: 94,
        overflow: "truncate",
        fontSize: 11,
      },
    },
    dataZoom:
      data.length > 7
        ? [
            {
              type: "slider",
              yAxisIndex: 0,
              right: 0,
              width: 12,
              startValue: 0,
              endValue: 6,
              showDetail: false,
              filterMode: "filter",
            },
          ]
        : [],
    series: [
      {
        name: seriesName,
        type: "bar",
        barMaxWidth: 22,
        data: data.map((item) => item.value),
        itemStyle: { color: theme.colors[6], borderRadius: [0, 3, 3, 0] },
        label: {
          show: true,
          position: "right",
          color: theme.text,
          fontSize: 10,
        },
      },
    ],
  });
}
function lines(
  dates: string[],
  series: { name: string; values: number[]; color: number }[],
): Option {
  return (theme) => ({
    legend: {
      top: 0,
      itemWidth: 14,
      itemHeight: 8,
      textStyle: { color: theme.muted, fontSize: 11 },
    },
    grid: { left: 38, right: 15, top: 45, bottom: 30 },
    tooltip: {
      trigger: "axis",
      renderMode: "richText",
      confine: true,
      backgroundColor: theme.surface,
      textStyle: { color: theme.text },
    },
    xAxis: {
      type: "category",
      boundaryGap: false,
      data: dates,
      axisTick: { show: false },
      axisLine: { lineStyle: { color: theme.line } },
      axisLabel: {
        color: theme.muted,
        fontSize: 10,
        formatter: (value: string) => value.slice(5),
      },
    },
    yAxis: {
      type: "value",
      minInterval: 1,
      axisLabel: { color: theme.muted, fontSize: 10 },
      splitLine: { lineStyle: { color: theme.line, type: "dashed" } },
    },
    series: series.map((item) => ({
      name: item.name,
      type: "line",
      smooth: false,
      showSymbol: dates.length <= 7,
      symbolSize: 5,
      data: item.values,
      lineStyle: { width: 2 },
      itemStyle: { color: theme.colors[item.color] },
    })),
  });
}

export default function Statistics({
  data,
  refresh,
  notify,
  workspaceId,
}: PageProps & { workspaceId: string }) {
  const [params, setParams] = useSearchParams();
  const tab = tabs.some(([id]) => id === params.get("view"))
    ? params.get("view")!
    : "work";
  const days = [7, 30, 90].includes(Number(params.get("days")))
    ? (Number(params.get("days")) as 7 | 30 | 90)
    : 30;
  const [now, setNow] = useState(Date.now);
  const dayStart = new Date(now).setHours(0, 0, 0, 0);
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const [refreshing, setRefreshing] = useState(false);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(timer);
  }, []);
  const stats = useMemo(
    () => buildStatistics(data, days, dayStart, timezone),
    [data, days, dayStart, timezone],
  );
  const bugs = useMemo(
    () => buildBugStatistics(data.bugs, days, dayStart, timezone),
    [data.bugs, days, dayStart, timezone],
  );
  const {
    data: mcp,
    isPending: mcpLoading,
    error: mcpError,
    refetch: refreshMcp,
  } = useQuery({
    queryKey: ["mcp", workspaceId],
    queryFn: () =>
      native ? command<McpSummary[]>("mcp_list") : Promise.resolve([]),
    enabled: !!workspaceId,
  });
  const m = stats.metrics,
    t = stats.taskMetrics;
  const options = useMemo(() => {
    const s = stats;
    const project: Option = (theme) => ({
      legend: { top: 0, textStyle: { color: theme.muted, fontSize: 11 } },
      grid: {
        left: 110,
        right: s.projectProgress.length > 7 ? 30 : 12,
        top: 42,
        bottom: 28,
      },
      xAxis: {
        type: "value",
        minInterval: 1,
        axisLabel: { color: theme.muted, fontSize: 10 },
        splitLine: { lineStyle: { color: theme.line } },
      },
      yAxis: {
        type: "category",
        inverse: true,
        data: s.projectProgress.map((p) => p.name),
        axisLabel: {
          color: theme.muted,
          width: 94,
          overflow: "truncate",
          fontSize: 11,
        },
        axisTick: { show: false },
        axisLine: { show: false },
      },
      dataZoom:
        s.projectProgress.length > 7
          ? [
              {
                type: "slider",
                yAxisIndex: 0,
                right: 0,
                width: 12,
                startValue: 0,
                endValue: 6,
                showDetail: false,
              },
            ]
          : [],
      series: (["todo", "doing", "done", "closed"] as const).map((key, i) => ({
        name: ["待做", "正在做", "已完成", "已关闭"][i],
        type: "bar",
        stack: "tasks",
        barMaxWidth: 22,
        itemStyle: { color: theme.colors[i] },
        data: s.projectProgress.map((p) => p[key]),
      })),
    });
    const heatmap: Option = (theme) => ({
      calendar: {
        range: [s.chatTrend[0].date, s.chatTrend.at(-1)!.date],
        top: 34,
        left: "center",
        cellSize: [30, 26],
        splitLine: { show: false },
        itemStyle: {
          color: theme.surface,
          borderColor: theme.surface,
          borderWidth: 3,
        },
        yearLabel: { show: false },
        monthLabel: { nameMap: "ZH", color: theme.muted, fontSize: 11 },
        dayLabel: {
          firstDay: 1,
          nameMap: ["日", "一", "二", "三", "四", "五", "六"],
          color: theme.muted,
          fontSize: 10,
        },
      },
      visualMap: {
        min: 0,
        max: Math.max(1, ...s.chatTrend.map((row) => row.userMessages)),
        orient: "horizontal",
        left: "center",
        bottom: 4,
        itemHeight: 120,
        itemWidth: 9,
        text: ["多", "少"],
        textStyle: { color: theme.muted },
        inRange: { color: [theme.surface, theme.colors[6]] },
      },
      series: [
        {
          name: "发送消息",
          type: "heatmap",
          coordinateSystem: "calendar",
          data: s.chatTrend.map((row) => [row.date, row.userMessages]),
        },
      ],
    });
    return {
      taskTrend: lines(
        s.taskTrend.map((r) => r.date),
        [
          {
            name: "新建任务",
            values: s.taskTrend.map((r) => r.created),
            color: 1,
          },
          {
            name: "完成任务",
            values: s.taskTrend.map((r) => r.completed),
            color: 2,
          },
        ],
      ),
      status: donut(s.taskStatus),
      priority: donut(s.taskPriority, [5, 1, 0]),
      source: donut(s.taskSource, [6, 4]),
      project,
      documents: donut(s.documentStatus, [2, 1, 4, 5, 0, 6]),
      formats: bars(s.documentFormats, "文档数"),
      categories: donut(s.appCategories),
      library: bars(
        s.libraryUsage.map((l) => ({ name: l.name, value: l.documents })),
        "文档数",
      ),
      chatTrend: lines(
        s.chatTrend.map((r) => r.date),
        [
          {
            name: "新建会话",
            values: s.chatTrend.map((r) => r.sessions),
            color: 1,
          },
          {
            name: "发送消息",
            values: s.chatTrend.map((r) => r.userMessages),
            color: 6,
          },
        ],
      ),
      agents: bars(s.agentUsage, "会话数"),
      capabilities: donut(s.modelCapabilities, [1, 4, 6]),
      providers: bars(s.providerModels, "模型数"),
      replies: donut(s.messageStatus, [2, 1, 0, 5, 4]),
      heatmap,
    };
  }, [stats]);
  function select(key: string, value: string) {
    const next = new URLSearchParams(params);
    next.set(key, value);
    setParams(next);
  }
  async function reload() {
    if (refreshing) return;
    setRefreshing(true);
    try {
      await refresh();
      await refreshMcp();
      setNow(Date.now());
    } catch {
      notify("统计数据刷新失败，请重试");
    } finally {
      setRefreshing(false);
    }
  }
  const activityExists = stats.taskTrend.some(
    (row) => row.created || row.completed,
  );
  return (
    <div className="statistics-page">
      <div className="page-heading">
        <div>
          <h1>统计</h1>
          <p>工作进展、资源与协作数据</p>
        </div>
        <div className="statistics-controls">
          <label>
            趋势范围
            <GlassSelect
              aria-label="统计趋势范围"
              value={String(days)}
              onValueChange={(value) => select("days", value)}
              options={[
                { value: "7", label: "近 7 天" },
                { value: "30", label: "近 30 天" },
                { value: "90", label: "近 90 天" },
              ]}
            />
          </label>
          <button
            type="button"
            className="icon-button"
            title="刷新统计"
            aria-label="刷新统计"
            disabled={refreshing}
            onClick={() => void reload()}
          >
            <RefreshCw
              size={17}
              className={refreshing ? "update-spinning" : ""}
            />
          </button>
        </div>
      </div>
      <dl className="statistics-summary">
        <div>
          <dt>任务总数</dt>
          <dd>{number(t.total)}</dd>
          <small>
            待处理 {number(t.open)} · 已完成 {number(t.done)}
          </small>
        </div>
        <div>
          <dt>项目</dt>
          <dd>{number(m.projects)}</dd>
          <small>
            禅道项目 {data.projects.filter((p) => p.source === "zentao").length}
          </small>
        </div>
        <div>
          <dt>知识文档</dt>
          <dd>{number(m.documents)}</dd>
          <small>
            {m.knowledge} 个知识库 · {bytes(m.documentBytes)}
          </small>
        </div>
        <div>
          <dt>聊天会话</dt>
          <dd>{number(m.conversations)}</dd>
          <small>
            消息 {number(m.messages)} · 关联知识库 {m.ragConversations}
          </small>
        </div>
      </dl>
      <div
        className="statistics-tabs"
        role="tablist"
        aria-label="统计维度"
        onKeyDown={(event) => {
          if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key))
            return;
          event.preventDefault();
          const index = tabs.findIndex(([id]) => id === tab);
          const next =
            event.key === "Home"
              ? 0
              : event.key === "End"
                ? tabs.length - 1
                : (index + (event.key === "ArrowRight" ? 1 : tabs.length - 1)) %
                  tabs.length;
          select("view", tabs[next][0]);
          document.getElementById(`statistics-tab-${tabs[next][0]}`)?.focus();
        }}
      >
        {tabs.map(([id, name, Icon]) => (
          <button
            key={id}
            id={`statistics-tab-${id}`}
            type="button"
            role="tab"
            aria-selected={tab === id}
            aria-controls="statistics-content"
            tabIndex={tab === id ? 0 : -1}
            onClick={() => select("view", id)}
          >
            <Icon size={16} />
            {name}
          </button>
        ))}
      </div>
      <div
        id="statistics-content"
        role="tabpanel"
        aria-labelledby={`statistics-tab-${tab}`}
      >
        {tab === "bugs" && (
          <>
            <div className="statistics-summary-strip">
              <span>
                BUG 总数<strong>{bugs.total}</strong>
              </span>
              <span>
                待解决<strong>{bugs.active}</strong>
              </span>
              <span>
                已解决<strong>{bugs.resolved}</strong>
              </span>
              <span>
                已关闭<strong>{bugs.closed}</strong>
              </span>
              <span>
                严重待解决<strong>{bugs.urgent}</strong>
              </span>
              <Link to="/bugs">管理 BUG</Link>
            </div>
            <p className="muted">
              统计当前本地 BUG 缓存；同步范围为指派给我的
              BUG。趋势依据远端创建／解决日期，不代表禅道全量历史，转派后再次同步会移出统计。
            </p>
            <div className="statistics-grid">
              <StatisticsChart
                title="BUG 状态分布"
                subtitle="当前同步缓存"
                option={donut(bugs.status, [3, 1, 4])}
                rows={countRows(bugs.status)}
                columns={["状态", "BUG 数"]}
                empty={
                  !bugs.total ? "暂无 BUG，请先在 BUG 修复中同步" : undefined
                }
              />
              <StatisticsChart
                title="BUG 严重程度"
                subtitle="1 级最高 · 包含全部状态"
                option={bars(bugs.severity, "BUG 数")}
                rows={countRows(bugs.severity)}
                columns={["严重程度", "BUG 数"]}
                empty={!bugs.total ? "暂无 BUG" : undefined}
              />
              <StatisticsChart
                title="BUG 创建与解决趋势"
                subtitle={`近 ${days} 天 · 当前缓存样本的远端业务日期`}
                option={lines(
                  bugs.trend.map((r) => r.date),
                  [
                    {
                      name: "创建",
                      values: bugs.trend.map((r) => r.opened),
                      color: 0,
                    },
                    {
                      name: "解决",
                      values: bugs.trend.map((r) => r.resolved),
                      color: 1,
                    },
                  ],
                )}
                rows={bugs.trend.map((r) => [r.date, r.opened, r.resolved])}
                columns={["日期", "创建", "解决"]}
                empty={
                  !bugs.trend.some((r) => r.opened || r.resolved)
                    ? "该时间段暂无记录"
                    : undefined
                }
              />
              <StatisticsChart
                title="BUG 产品分布"
                subtitle="按数量排序"
                option={bars(bugs.products, "BUG 数")}
                rows={countRows(bugs.products)}
                columns={["产品", "BUG 数"]}
                empty={!bugs.total ? "暂无 BUG" : undefined}
              />
            </div>
          </>
        )}
        {tab === "work" && (
          <>
            <div className="statistics-summary-strip">
              <span>
                完成率<strong>{percent(t.completionRate)}</strong>
              </span>
              <span>
                已逾期<strong>{t.overdue}</strong>
              </span>
              <span>
                未排期<strong>{t.unscheduled}</strong>
              </span>
              <span>
                已关闭<strong>{t.closed}</strong>
              </span>
              <Link to="/tasks">查看任务</Link>
            </div>
            <div className="statistics-grid">
              <StatisticsChart
                title="任务变化趋势"
                subtitle={`近 ${days} 天 · 按本地自然日`}
                option={options.taskTrend}
                rows={stats.taskTrend.map((r) => [
                  r.date,
                  r.created,
                  r.completed,
                ])}
                columns={["日期", "新建", "完成"]}
                empty={!activityExists ? "该时间段暂无任务活动" : undefined}
              />
              <StatisticsChart
                title="任务状态分布"
                subtitle="当前全部任务"
                option={options.status}
                rows={countRows(stats.taskStatus)}
                columns={["状态", "任务数"]}
                empty={!t.total ? "暂无任务" : undefined}
              />
              <StatisticsChart
                title="项目任务进度"
                subtitle="按任务数量排序 · 包含未关联项目"
                option={options.project}
                rows={stats.projectProgress.map((p) => [
                  p.name,
                  p.todo,
                  p.doing,
                  p.done,
                  p.closed,
                ])}
                columns={["项目", "待做", "正在做", "完成", "关闭"]}
                empty={!t.total ? "暂无项目任务" : undefined}
              />
              <StatisticsChart
                title="任务优先级"
                subtitle="当前全部任务"
                option={options.priority}
                rows={countRows(stats.taskPriority)}
                columns={["优先级", "任务数"]}
                empty={!t.total ? "暂无任务" : undefined}
              />
              <StatisticsChart
                title="任务来源"
                subtitle="本地任务与禅道同步条目"
                option={options.source}
                rows={countRows(stats.taskSource)}
                columns={["来源", "条目数"]}
                empty={!t.total ? "暂无任务" : undefined}
              />
              <section className="statistics-panel">
                <header className="statistics-panel-heading">
                  <div>
                    <h2>项目概况</h2>
                    <p>当前项目与关联任务</p>
                  </div>
                  <Link to="/projects">全部项目</Link>
                </header>
                <div className="statistics-data-table">
                  <table>
                    <thead>
                      <tr>
                        <th>项目</th>
                        <th>任务数</th>
                        <th>完成率</th>
                      </tr>
                    </thead>
                    <tbody>
                      {stats.projectProgress.map((p) => (
                        <tr key={p.id}>
                          <td>{p.name}</td>
                          <td>{p.total}</td>
                          <td>
                            {percent(
                              p.total - p.closed > 0
                                ? p.done / (p.total - p.closed)
                                : null,
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  {!stats.projectProgress.length && (
                    <p className="statistics-footnote">暂无项目</p>
                  )}
                </div>
              </section>
            </div>
            <p className="statistics-footnote">
              完成率不计已关闭任务；逾期按截止日期统计未完成任务。趋势仅包含有记录时间的数据，完成曲线按当前已完成任务的完成时间计算；重新打开或删除任务后会相应变化。
            </p>
          </>
        )}
        {tab === "resources" && (
          <>
            <div className="statistics-summary-strip">
              <span>
                应用<strong>{m.apps}</strong>
              </span>
              <span>
                收藏<strong>{m.favorites}</strong>
              </span>
              <span>
                已索引文档<strong>{m.readyDocuments}</strong>
              </span>
              <span>
                知识分块<strong>{number(m.chunks)}</strong>
              </span>
              <span>
                已启用技能
                <strong>
                  {m.enabledSkills} / {m.skills}
                </strong>
              </span>
              <span>
                已关联 Agent 的技能<strong>{m.boundSkills}</strong>
              </span>
            </div>
            <div className="statistics-grid">
              <StatisticsChart
                title="文档处理状态"
                subtitle={`${m.knowledge} 个知识库 · ${bytes(m.documentBytes)} 原件逻辑大小`}
                option={options.documents}
                rows={countRows(stats.documentStatus)}
                columns={["状态", "文档数"]}
                empty={!m.documents ? "暂无知识文档" : undefined}
              />
              <StatisticsChart
                title="知识库文档分布"
                subtitle="按文档数量排序"
                option={options.library}
                rows={stats.libraryUsage.map((l) => [
                  l.name,
                  l.documents,
                  l.ready,
                  bytes(l.bytes),
                  l.chunks,
                ])}
                columns={["知识库", "文档", "已索引", "原件大小", "分块"]}
                empty={!m.documents ? "暂无知识文档" : undefined}
              />
              <StatisticsChart
                title="文档格式"
                subtitle="按文件扩展名统计"
                option={options.formats}
                rows={countRows(stats.documentFormats)}
                columns={["格式", "文档数"]}
                empty={!m.documents ? "暂无知识文档" : undefined}
              />
              <StatisticsChart
                title="应用分类"
                subtitle={`${m.apps} 个应用 · ${m.favorites} 个收藏`}
                option={options.categories}
                rows={countRows(stats.appCategories)}
                columns={["分类", "应用数"]}
                empty={!m.apps ? "暂无应用" : undefined}
              />
            </div>
            <p className="statistics-footnote">
              资源图表展示当前存量，不受趋势范围影响。原件逻辑大小按文档记录累加，不代表去重后文件占用、索引或整个数据库的磁盘大小。
            </p>
          </>
        )}
        {tab === "ai" && (
          <>
            <div className="statistics-summary-strip">
              <span>
                已启用供应商
                <strong>
                  {m.enabledProviders} / {m.providers}
                </strong>
              </span>
              <span>
                已启用模型
                <strong>
                  {m.enabledModels} / {m.models}
                </strong>
              </span>
              <span>
                已启用 Agent
                <strong>
                  {m.enabledAgents} / {m.agents}
                </strong>
              </span>
              <span>
                助手回复<strong>{m.assistantMessages}</strong>
              </span>
              <span>
                失败或中断<strong>{m.failedMessages}</strong>
              </span>
            </div>
            <div className="statistics-grid">
              <StatisticsChart
                title="聊天活动趋势"
                subtitle={`近 ${days} 天 · 按创建时间`}
                option={options.chatTrend}
                rows={stats.chatTrend.map((r) => [
                  r.date,
                  r.sessions,
                  r.userMessages,
                  r.assistantMessages,
                ])}
                columns={["日期", "新会话", "发送消息", "助手回复"]}
                empty={
                  !stats.chatTrend.some((r) => r.sessions || r.userMessages)
                    ? "该时间段暂无聊天活动"
                    : undefined
                }
              />
              <StatisticsChart
                title="Agent 会话分布"
                subtitle="当前保存的全部会话"
                option={options.agents}
                rows={countRows(stats.agentUsage)}
                columns={["Agent", "会话数"]}
                empty={!m.conversations ? "暂无聊天会话" : undefined}
              />
              <StatisticsChart
                title="消息活跃日历"
                subtitle={`近 ${days} 天 · 每日发送的用户消息`}
                option={options.heatmap}
                rows={stats.chatTrend.map((r) => [r.date, r.userMessages])}
                columns={["日期", "发送消息"]}
                empty={
                  !stats.chatTrend.some((r) => r.userMessages)
                    ? "该时间段暂无发送消息"
                    : undefined
                }
                wide
              />
              <StatisticsChart
                title="模型能力分布"
                subtitle="包含已启用与停用模型"
                option={options.capabilities}
                rows={countRows(stats.modelCapabilities)}
                columns={["能力", "模型数"]}
                empty={!m.models ? "暂无模型配置" : undefined}
              />
              <StatisticsChart
                title="供应商模型数量"
                subtitle="当前模型配置"
                option={options.providers}
                rows={countRows(stats.providerModels)}
                columns={["供应商", "模型数"]}
                empty={!m.models ? "暂无模型配置" : undefined}
              />
              <StatisticsChart
                title="助手回复状态"
                subtitle="当前保存的助手消息"
                option={options.replies}
                rows={countRows(stats.messageStatus)}
                columns={["状态", "消息数"]}
                empty={!m.assistantMessages ? "暂无助手回复" : undefined}
              />
              <section className="statistics-panel">
                <header className="statistics-panel-heading">
                  <div>
                    <h2>连接配置</h2>
                    <p>禅道与 MCP · 启用不代表实时连通</p>
                  </div>
                  <Link to="/settings">管理连接</Link>
                </header>
                {mcpError && (
                  <p role="alert" className="error">
                    MCP 配置读取失败{" "}
                    <button onClick={() => void refreshMcp()}>重试</button>
                  </p>
                )}
                <div className="statistics-connection-table">
                  <table>
                    <thead>
                      <tr>
                        <th>类型</th>
                        <th>配置数</th>
                        <th>已启用</th>
                      </tr>
                    </thead>
                    <tbody>
                      <tr>
                        <td>禅道</td>
                        <td>{data.connections.length}</td>
                        <td>
                          {data.connections.filter((c) => c.enabled).length}
                        </td>
                      </tr>
                      <tr>
                        <td>MCP</td>
                        <td>
                          {mcpError
                            ? "读取失败"
                            : mcpLoading
                              ? "加载中"
                              : (mcp?.length ?? 0)}
                        </td>
                        <td>
                          {mcpError || mcpLoading
                            ? "暂无"
                            : (mcp?.filter((c) => c.enabled).length ?? 0)}
                        </td>
                      </tr>
                    </tbody>
                  </table>
                </div>
                <p className="statistics-footnote">
                  有同步记录的禅道连接{" "}
                  {data.connections.filter((c) => !!c.lastSync).length} 个。
                </p>
              </section>
            </div>
            <p className="statistics-footnote">
              聊天图表统计本地保存的会话与消息，不包含已删除记录。配置数不代表调用次数；当前未采集
              Token 用量、费用或 MCP 调用量。
            </p>
          </>
        )}
      </div>
    </div>
  );
}
