import React, { useEffect, useState, lazy, Suspense } from "react";
import TopbarClock from "./components/TopbarClock";
import { createRoot } from "react-dom/client";
import {
  HashRouter,
  NavLink,
  Routes,
  Route,
  useLocation,
  Navigate,
  useNavigate,
} from "react-router-dom";
import {
  QueryClient,
  QueryClientProvider,
  useQuery,
} from "@tanstack/react-query";
import {
  House,
  FolderKanban,
  SquareCheckBig,
  Bug,
  CalendarDays,
  LayoutGrid,
  MessagesSquare,
  Settings2,
  Search,
  RefreshCw,
  ChartNoAxesCombined,
  Mail as MailIcon,
} from "lucide-react";
import { api, native, emptySnapshot, appVersion } from "./lib/api";
import {
  getAppearance,
  initializeAppearance,
  type AppearanceMode,
} from "./lib/appearance";
import { getCurrentWindow } from "@tauri-apps/api/window";
import UpdatePopover from "./components/UpdatePopover";
import { listen } from "@tauri-apps/api/event";
import { mailApi, mailKeys } from "./lib/mail";
import "./features/mail.css";
const Mail = lazy(() => import("./features/Mail"));
const Work = lazy(() => import("./features/Work"));
const Bugs = lazy(() => import("./features/Bugs"));
const Projects = lazy(() =>
  import("./features/Work").then((m) => ({ default: m.Projects })),
);
const Apps = lazy(() =>
  import("./features/Resources").then((m) => ({ default: m.Apps })),
);
const Settings = lazy(() => import("./features/Settings"));
const Chat = lazy(() => import("./features/Chat"));
const Statistics = lazy(() => import("./features/Statistics"));
import "./styles.css";
import "./glass.css";
import "./theme.css";
let nativeAppearance: AppearanceMode | undefined;
const disposeAppearance = initializeAppearance(() => {
  const mode = getAppearance().mode;
  if (native && nativeAppearance !== mode) {
    nativeAppearance = mode;
    // null 交还系统控制，避免将跟随系统固定成当前解析出的颜色。
    void getCurrentWindow()
      .setTheme(mode === "system" ? null : mode)
      .catch(() => {
        nativeAppearance = undefined;
      });
  }
});
if (import.meta.hot) import.meta.hot.dispose(disposeAppearance);
const client = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: true } },
});
const navigation = [
  ["/", "总览", House],
  ["/projects", "项目", FolderKanban],
  ["/tasks", "我的任务", SquareCheckBig],
  ["/bugs", "BUG 修复", Bug],
  ["/calendar", "日历", CalendarDays],
  ["/apps", "应用", LayoutGrid],
  ["/chat", "聊天", MessagesSquare],
  ["/mail", "邮件", MailIcon],
  ["/statistics", "统计", ChartNoAxesCombined],
] as const;
function App() {
  const {
    data = emptySnapshot,
    error,
    refetch,
  } = useQuery({ queryKey: ["workspace"], queryFn: api.snapshot });
  const {
    data: info = {
      dataRoot: "~/.perch",
      defaultRoot: "~/.perch",
      workspaceId: "",
      version: appVersion,
    },
    refetch: refreshInfo,
  } = useQuery({ queryKey: ["storage"], queryFn: api.info });
  const [search, setSearch] = useState(""),
    [toast, setToast] = useState("");
  const location = useLocation();
  const navigate = useNavigate();
  const { data: mailUnread } = useQuery({
    queryKey: mailKeys.unread,
    queryFn: mailApi.unread,
    refetchInterval: 15000,
  });
  useEffect(() => {
    void client.resetQueries({ queryKey: ["mail"] });
  }, [info.workspaceId, info.dataRoot]);
  useEffect(() => {
    if (!native) return;
    let stopped = false;
    const disposers: (() => void)[] = [];
    const register = async () => {
      const offMail = await listen<{ workspaceId: string }>(
        "mail-updated",
        (event) => {
          if (event.payload.workspaceId === info.workspaceId)
            void client.invalidateQueries({ queryKey: ["mail"] });
        },
      );
      if (stopped) offMail();
      else disposers.push(offMail);
      const offZentao = await listen<{ workspaceId: string }>(
        "zentao-updated",
        (event) => {
          if (event.payload.workspaceId === info.workspaceId)
            void client.invalidateQueries({ queryKey: ["workspace"] });
        },
      );
      if (stopped) offZentao();
      else disposers.push(offZentao);
      const offTray = await listen<string>("tray-navigate", (event) => {
        if (event.payload === "/tasks") navigate("/tasks");
      });
      if (stopped) offTray();
      else disposers.push(offTray);
    };
    void register().catch(() => {});
    return () => {
      stopped = true;
      disposers.forEach((dispose) => dispose());
    };
  }, [info.workspaceId, navigate]);
  const notify = (text: string) => {
    setToast(text);
    setTimeout(() => setToast(""), 6000);
  };
  const props = { data, refresh: refetch, notify };
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <NavLink to="/" aria-label="栖点首页">
            <span className="brandmark">
              <img src="/workbench-logo.svg" alt="" />
            </span>
          </NavLink>
          <div>
            <div className="brand-title">
              <NavLink to="/">栖点</NavLink>{" "}
              <UpdatePopover version={info.version} />
            </div>
            <small>PERCH</small>
          </div>
        </div>
        <nav>
          {navigation.map(([path, label, Icon]) => (
            <NavLink
              end={path === "/"}
              to={path}
              key={path}
              onClick={() => setSearch("")}
            >
              <Icon size={18} />
              <span>{label}</span>
              {path === "/bugs" && (
                <small title="待解决 BUG">
                  {
                    (data.bugs || []).filter((bug) => bug.status === "active")
                      .length
                  }
                </small>
              )}
              {path === "/mail" && !!mailUnread?.total && (
                <small
                  className="mail-nav-badge"
                  aria-label={`${mailUnread.total} 封未读邮件`}
                >
                  {mailUnread.total > 99 ? "99+" : mailUnread.total}
                </small>
              )}
              {path === "/tasks" && (
                <small>
                  {
                    data.tasks.filter(
                      (t) => t.status === "todo" || t.status === "doing",
                    ).length
                  }
                </small>
              )}
            </NavLink>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <NavLink to="/settings">
            <Settings2 size={18} />
            <span>设置与连接</span>
          </NavLink>
          <div className="identity">
            <span>我</span>
            <div>
              我的工作空间<small>本地个人空间</small>
            </div>
          </div>
        </div>
      </aside>
      <div className="workspace">
        <header className="topbar">
          <span>
            我的空间 <span className="muted"> / </span>{" "}
            <b>
              {navigation.find((n) => n[0] === location.pathname)?.[1] ||
                "设置"}
            </b>
          </span>
          <TopbarClock />
          <label className="global-search">
            <Search size={16} />
            <input
              aria-label="全局搜索"
              placeholder="搜索任务、BUG、项目或应用"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </label>
          <button
            className="icon-button"
            title="刷新数据"
            onClick={() => void refetch()}
          >
            <RefreshCw size={16} />
          </button>
        </header>
        {!native && (
          <div className="preview-notice">
            浏览器界面预览 · 数据操作请在桌面应用中进行
          </div>
        )}
        {error ? (
          <div className="error-panel" role="alert">
            <h2>无法读取工作空间</h2>
            <p>{error.message}</p>
            <button onClick={() => void refetch()}>重新读取</button>
          </div>
        ) : (
          <main
            className={
              location.pathname === "/chat"
                ? "chat-page"
                : location.pathname === "/mail"
                  ? "mail-page"
                  : ""
            }
          >
            <Suspense
              fallback={
                <div className="empty" role="status">
                  正在加载…
                </div>
              }
            >
              <Routes>
                <Route
                  path="/"
                  element={<Work {...props} page="overview" search={search} />}
                />
                <Route
                  path="/tasks"
                  element={<Work {...props} search={search} />}
                />
                <Route
                  path="/projects"
                  element={<Projects {...props} search={search} />}
                />
                <Route
                  path="/bugs"
                  element={<Bugs {...props} search={search} />}
                />
                <Route
                  path="/calendar"
                  element={<Work key={`${info.workspaceId}:${info.dataRoot}`} {...props} page="calendar" search={search} />}
                />
                <Route
                  path="/apps"
                  element={<Apps {...props} search={search} />}
                />
                <Route
                  path="/agents"
                  element={<Navigate to="/settings?tab=agents" replace />}
                />
                <Route
                  path="/skills"
                  element={<Navigate to="/settings?tab=skills" replace />}
                />
                <Route
                  path="/knowledge"
                  element={<Navigate to="/settings?tab=knowledge" replace />}
                />
                <Route path="/chat" element={<Chat {...props} />} />
                <Route
                  path="/mail"
                  element={<Mail key={info.workspaceId} notify={notify} />}
                />
                <Route
                  path="/statistics"
                  element={
                    <Statistics {...props} workspaceId={info.workspaceId} />
                  }
                />
                <Route
                  path="/settings"
                  element={
                    <Settings
                      {...props}
                      info={info}
                      onInfo={refreshInfo}
                      search={search}
                    />
                  }
                />
              </Routes>
            </Suspense>
          </main>
        )}
        <footer className="statusbar">
          <span>
            <i />
            {native ? "本地工作空间" : "界面预览"}
          </span>
          <span>
            SQLite ·{" "}
            {data.connections.length
              ? "已配置 " + data.connections.length + " 个禅道连接"
              : "禅道未连接"}
          </span>
        </footer>
      </div>
      {toast && (
        <div className="toast" role="status" onClick={() => setToast("")}>
          {toast}
        </div>
      )}
    </div>
  );
}
createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={client}>
      <HashRouter>
        <App />
      </HashRouter>
    </QueryClientProvider>
  </React.StrictMode>,
);
