import { invoke, isTauri } from "@tauri-apps/api/core";
import { version } from "../../package.json";
import type { EntityKind, Base, Snapshot, StorageInfo } from "./types";
export const native = isTauri();
export const appVersion = version;
export const emptySnapshot: Snapshot = {
  bugs: [],
  tasks: [],
  projects: [],
  apps: [],
  categories: [],
  providers: [],
  models: [],
  agents: [],
  skills: [],
  knowledge: [],
  documents: [],
  conversations: [],
  messages: [],
  connections: [],
};
export async function command<T>(
  name: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  if (!native) throw new Error("此功能需要在桌面应用中使用");
  try {
    return await invoke<T>(name, args);
  } catch (error) {
    throw new Error(typeof error === "string" ? error : JSON.stringify(error));
  }
}
export const api = {
  snapshot: () =>
    native
      ? command<Snapshot>("workspace_snapshot")
      : Promise.resolve(emptySnapshot),
  info: () =>
    native
      ? command<StorageInfo>("storage_info")
      : Promise.resolve({
          dataRoot: "~/.perch",
          defaultRoot: "~/.perch",
          workspaceId: "preview",
          version: appVersion,
        }),
  save: <T extends Base>(kind: EntityKind, value: T) =>
    command<T>(`save_${kind}`, { value }),
  remove: (kind: EntityKind, id: string, revision?: number) =>
    command<void>(`delete_${kind}`, { id, revision }),
};
