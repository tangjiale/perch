import { invoke, isTauri } from "@tauri-apps/api/core";
import { version } from "../../package.json";
import type { BackupSaveInput, BackupState } from "./backup-types";
import type { ExchangeRateSnapshot } from "./exchange-rates";
import type { EntityKind, Base, Snapshot, StorageInfo, ZentaoTaskAssignment, ZentaoBugHistory, ZentaoAssignee, NatappState, NatappSaveInput, NatappLogs } from "./types";
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
  exchangeRates: (base: string) => command<ExchangeRateSnapshot>("get_exchange_rates", { base }),
  backupState: () => command<BackupState>("backup_state"),
  backupSave: (value: BackupSaveInput) =>
    command<BackupState>("backup_save", { value }),
  backupTest: (value: BackupSaveInput) =>
    command<void>("backup_test", { value }),
  backupRefresh: () => command<BackupState>("backup_refresh"),
  backupCreate: (id?: string) => command<BackupState>("backup_create", { id }),
  backupVerify: (id: string) => command<BackupState>("backup_verify", { id }),
  backupRetry: (id: string) => command<BackupState>("backup_retry", { id }),
  backupDownload: (id: string, destination: string) => command<void>("backup_download", { id, destination }),
  backupRestore: (id: string) => command<StorageInfo>("backup_restore", { id }),
  natappState: () => command<NatappState>("natapp_state"),
  natappInstall: () => command<NatappState>("natapp_install"),
  natappSave: (value: NatappSaveInput) => command<NatappState>("natapp_save", { value }),
  natappDelete: (id: string, revision: number) => command<NatappState>("natapp_delete", { id, revision }),
  natappStart: (id: string) => command<NatappState>("natapp_start", { id }),
  natappStop: (id: string) => command<NatappState>("natapp_stop", { id }),
  natappLogs: (id: string) => command<NatappLogs>("natapp_logs", { id }),
  bugAssignees: (bugId: string) => command<ZentaoAssignee[]>("zentao_bug_assignees", { bugId }),
  bugHistory: (bugId: string) => command<ZentaoBugHistory>("zentao_bug_history", { bugId }),
  copyImage: (dataUrl: string) => command<void>("image_clipboard_write", { dataUrl }),
  downloadImage: (dataUrl: string) => command<boolean>("image_download", { dataUrl }),
  taskReminderPermission: () => command<boolean>("task_reminder_permission"),
  taskAssignment: (taskId: string) =>
    command<ZentaoTaskAssignment>("zentao_task_assignment", { taskId }),
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
