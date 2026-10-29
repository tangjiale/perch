import { useCallback, useContext, useSyncExternalStore } from "react";
import { QueryClientContext } from "@tanstack/react-query";
import { command, native } from "./api";
export interface AssignmentNotification {
  id: string;
  kind: "task" | "bug";
  entityId: string;
  title: string;
  createdAt: number;
}
export interface AssignmentInbox {
  workspaceId: string;
  generation: string;
  unreadCount: number;
  taskCount: number;
  bugCount: number;
  items: AssignmentNotification[];
}
export const assignmentKey = (workspaceId: string) => ["assignment-notifications", workspaceId] as const;
export const emptyInbox = (workspaceId: string): AssignmentInbox => ({workspaceId, generation: "", unreadCount: 0, taskCount: 0, bugCount: 0, items: []});
export const assignmentApi = {
  list: (workspaceId: string) => native ? command<AssignmentInbox>("assignment_notification_list", {workspaceId}) : Promise.resolve(emptyInbox(workspaceId)),
  read: (workspaceId: string, id: string, generation: string) => command<void>("assignment_notification_read", {workspaceId, id, generation}),
  readEntity: (workspaceId: string, kind: AssignmentNotification["kind"], entityId: string, generation: string) => command<void>("assignment_notification_read_entity", {workspaceId, kind, entityId, generation}),
};
export const assignmentPath = (item: Pick<AssignmentNotification, "kind" | "entityId"> & {id?: string}) => {
  const path = item.kind === "task" ? `/tasks?taskId=${encodeURIComponent(item.entityId)}` : `/bugs?bugId=${encodeURIComponent(item.entityId)}`;
  return item.id ? `${path}&notificationId=${encodeURIComponent(item.id)}` : path;
};

// 可独立渲染的详情 fixture 没有查询上下文，此时不执行通知写入。
export function useAssignmentGeneration(workspaceId: string) {
  const client = useContext(QueryClientContext);
  const subscribe = useCallback((changed: () => void) => client?.getQueryCache().subscribe(changed) ?? (() => {}), [client]);
  const snapshot = useCallback(() => client?.getQueryData<AssignmentInbox>(assignmentKey(workspaceId))?.generation || "", [client, workspaceId]);
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}
