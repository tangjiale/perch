import { useEffect, useRef, useState } from "react";
import * as Popover from "@radix-ui/react-popover";
import { Bell, Bug, SquareCheckBig, X } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { native } from "../lib/api";
import { assignmentApi, assignmentKey, assignmentPath, type AssignmentNotification } from "../lib/assignment-notifications";
import "./assignment-notifications.css";

export default function AssignmentNotifications({workspaceId, notify}: {workspaceId: string; notify: (message: string) => void}) {
  const client = useQueryClient();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [bubbles, setBubbles] = useState<AssignmentNotification[]>([]);
  const currentWorkspace = useRef(workspaceId);
  currentWorkspace.current = workspaceId;
  const {data, isPending, error, refetch} = useQuery({queryKey: assignmentKey(workspaceId), queryFn: () => assignmentApi.list(workspaceId), enabled: !!workspaceId});
  useEffect(() => {
    setOpen(false); setBubbles([]); setBusy(null);
    if (!native || !workspaceId) return;
    let stopped = false;
    const off: (() => void)[] = [];
    const register = async () => {
      const add = async <T,>(name: string, handle: (payload: T) => void) => {
        const dispose = await listen<T>(name, event => {if (!stopped) handle(event.payload);});
        if (stopped) dispose(); else off.push(dispose);
      };
      await add<{workspaceId: string}>("assignment-notifications-updated", payload => {
        if (payload.workspaceId === workspaceId) void client.invalidateQueries({queryKey: assignmentKey(workspaceId)});
      });
      await add<{workspaceId: string; items: AssignmentNotification[]}>("assignment-notification-new", payload => {
        if (payload.workspaceId !== workspaceId) return;
        setBubbles(previous => [...payload.items, ...previous.filter(item => !payload.items.some(next => next.id === item.id))].slice(0, 3));
        void client.invalidateQueries({queryKey: assignmentKey(workspaceId)});
      });
      await add<AssignmentNotification & {workspaceId: string}>("assignment-notification-open", payload => {
        if (payload.workspaceId !== workspaceId) return;
        setBubbles(previous => previous.filter(item => item.id !== payload.id));
        setOpen(false);
        void client.invalidateQueries({queryKey: assignmentKey(workspaceId)});
        void client.invalidateQueries({queryKey: ["workspace"]});
        navigate(assignmentPath(payload));
      });
    };
    void register().catch(() => {});
    return () => {stopped = true; off.forEach(dispose => dispose());};
  }, [workspaceId, client, navigate]);
  // 详情页或原生通知已读后，及时撤下仍在展示的同条气泡。
  useEffect(() => {
    if (!data || data.workspaceId !== workspaceId) return;
    setBubbles(previous => previous.filter(item => data.items.some(unread => unread.id === item.id)));
  }, [data, workspaceId]);
  useEffect(() => {
    if (!bubbles.length) return;
    const timer = window.setTimeout(() => setBubbles([]), 8000);
    return () => window.clearTimeout(timer);
  }, [bubbles]);
  async function showItem(item: AssignmentNotification) {
    if (busy) return;
    if (!data?.generation) { notify("提醒尚未读取完成，请稍后重试"); return; }
    const origin = workspaceId;
    setBusy(item.id);
    try {
      await assignmentApi.read(origin, item.id, data.generation);
      if (currentWorkspace.current !== origin) return;
      setBubbles(previous => previous.filter(value => value.id !== item.id));
      setOpen(false);
      await client.invalidateQueries({queryKey: assignmentKey(origin)});
      if (currentWorkspace.current === origin) navigate(assignmentPath(item));
    } catch (error) {
      if (currentWorkspace.current === origin) notify(`无法打开指派提醒：${error instanceof Error ? error.message : String(error)}`);
    } finally {if (currentWorkspace.current === origin) setBusy(null);}
  }
  function itemContent(item: AssignmentNotification) {
    const Icon = item.kind === "task" ? SquareCheckBig : Bug;
    return <><Icon size={18}/><span><small>新{item.kind === "task" ? "任务" : "BUG"}指派给你</small><strong>{item.title}</strong></span></>;
  }
  return <>
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild><button className="icon-button assignment-bell" aria-label={`指派提醒${data?.unreadCount ? `，${data.unreadCount} 条未读` : ""}`} title="指派提醒"><Bell size={18}/>{!!data?.unreadCount && <span className="assignment-count">{data.unreadCount > 99 ? "99+" : data.unreadCount}</span>}</button></Popover.Trigger>
      <Popover.Portal><Popover.Content className="assignment-popover" sideOffset={10} align="end" aria-label="未读指派提醒">
        <div className="assignment-heading"><strong>指派提醒</strong><span>{data?.unreadCount || 0} 条未读</span><Popover.Close className="icon-button" aria-label="关闭提醒列表"><X size={16}/></Popover.Close></div>
        {error ? <div className="assignment-empty" role="alert">提醒读取失败<button onClick={() => void refetch()}>重新读取</button></div> : isPending ? <p className="assignment-empty">正在读取提醒…</p> : !data?.items.length ? <p className="assignment-empty">暂无未读指派提醒</p> : <div className="assignment-list">{data.items.map(item => <button disabled={!!busy} className="assignment-item" key={item.id} onClick={() => void showItem(item)}>{itemContent(item)}<i aria-label="未读"/></button>)}</div>}
        <p className="assignment-hint">点击查看后清除该条未读提醒</p>
      </Popover.Content></Popover.Portal>
    </Popover.Root>
    <div className="assignment-bubbles" hidden={open} aria-live="polite">{bubbles.map(item => <div className="assignment-bubble" key={item.id}><button className="assignment-item" disabled={!!busy} onClick={() => void showItem(item)}>{itemContent(item)}</button><button className="icon-button" aria-label="关闭气泡，保留未读" onClick={() => setBubbles(previous => previous.filter(value => value.id !== item.id))}><X size={16}/></button></div>)}</div>
  </>;
}
