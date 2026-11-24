import type { Conversation } from "./types";

export interface ChatGroup {
  id: string;
  name: string;
  collapsed?: boolean;
}

export const CHAT_GROUPS_KEY = "perch-chat-groups";

export function chatGroupsStorageKey(workspaceId?: string): string {
  return workspaceId ? `${CHAT_GROUPS_KEY}:${workspaceId}` : CHAT_GROUPS_KEY;
}

function parseChatGroups(raw: string | null): ChatGroup[] {
  if (!raw) return [];
  try {
    const value = JSON.parse(raw);
    return Array.isArray(value)
      ? value.filter(
          (item): item is ChatGroup =>
            !!item &&
            typeof item.id === "string" &&
            typeof item.name === "string",
        )
      : [];
  } catch {
    return [];
  }
}

export function loadChatGroups(
  workspaceId?: string,
  conversations: Pick<Conversation, "groupId">[] = [],
  storage: Pick<Storage, "getItem"> | undefined = typeof localStorage !== "undefined"
    ? localStorage
    : undefined,
): ChatGroup[] {
  if (!storage) return [];
  const scopedKey = chatGroupsStorageKey(workspaceId);
  const scopedRaw = storage.getItem(scopedKey);
  if (scopedRaw !== null || !workspaceId) {
    return parseChatGroups(scopedRaw);
  }
  // 兼容旧版未按工作空间隔离的分组存储：仅保留当前工作空间实际引用的分组，避免跨空间串组。
  const legacy = parseChatGroups(storage.getItem(CHAT_GROUPS_KEY));
  if (!legacy.length) return [];
  const usedIds = new Set(
    conversations
      .map((item) => item.groupId)
      .filter((id): id is string => Boolean(id)),
  );
  return usedIds.size ? legacy.filter((group) => usedIds.has(group.id)) : [];
}

export function mergeConversationGroups(
  stored: ChatGroup[],
  conversations: Pick<Conversation, "groupId" | "groupName">[],
): ChatGroup[] {
  const known = new Set(stored.map((group) => group.id));
  const recovered: ChatGroup[] = [];
  for (const conversation of conversations) {
    if (conversation.groupId && !known.has(conversation.groupId)) {
      known.add(conversation.groupId);
      recovered.push({
        id: conversation.groupId,
        name: conversation.groupName?.trim() || "未命名项目",
      });
    }
  }
  return recovered.length ? [...stored, ...recovered] : stored;
}
