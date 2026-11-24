import { describe, expect, it } from "vitest";
import {
  CHAT_GROUPS_KEY,
  chatGroupsStorageKey,
  loadChatGroups,
  mergeConversationGroups,
} from "./chat-groups";

function memoryStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
  };
}

describe("chat-groups", () => {
  it("recovers missing groups from restored conversations so grouped chats never vanish", () => {
    const merged = mergeConversationGroups(
      [{ id: "g1", name: "本地已有项目" }],
      [
        { groupId: "g1", groupName: "本地已有项目" },
        { groupId: "g2", groupName: "备份恢复项目" },
        { groupId: "g3" },
        {},
      ],
    );
    expect(merged).toEqual([
      { id: "g1", name: "本地已有项目" },
      { id: "g2", name: "备份恢复项目" },
      { id: "g3", name: "未命名项目" },
    ]);
  });

  it("isolates groups per workspace and only migrates referenced legacy groups", () => {
    const storage = memoryStorage({
      [CHAT_GROUPS_KEY]: JSON.stringify([
        { id: "ws1-group", name: "空间一项目" },
        { id: "ws2-group", name: "空间二项目" },
      ]),
      [chatGroupsStorageKey("ws-b")]: JSON.stringify([
        { id: "b-only", name: "独立项目" },
      ]),
    });

    expect(
      loadChatGroups("ws-a", [{ groupId: "ws1-group" }], storage),
    ).toEqual([{ id: "ws1-group", name: "空间一项目" }]);
    expect(loadChatGroups("ws-a", [], storage)).toEqual([]);
    expect(
      loadChatGroups("ws-b", [{ groupId: "ws1-group" }], storage),
    ).toEqual([{ id: "b-only", name: "独立项目" }]);
  });
});
