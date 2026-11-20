import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import type { Meeting } from "./meetings";
import type { PromptEntry, PromptProgress, PromptRequest, PromptResult } from "./prompt-optimizer";
import { version } from "../../package.json";
import type { BackupSaveInput, BackupState } from "./backup-types";
import type { ExchangeRateSnapshot } from "./exchange-rates";
import type { AppCredentials, AppExcelPreview, AppExcelSource, Application, BrowserBookmarkAppInput, BrowserBookmarkImportResult, BrowserBookmarks } from "./types";
import type { SharedTable, SharedTableGroup, SharedTableGroupInput, SharedTableInput } from "./types";
import type { BugDraft, ZentaoBug, ZentaoBugCreateOptions, ZentaoBugDraft } from "./types";
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
export interface MeetingRealtimeEvent {
  sessionId: string; workspaceId: string; meetingId: string;
  type: "delta" | "error"; text?: string; message?: string;
}
export const api = {
  getAppCredentials: (workspaceId: string, appId: string) =>
    command<AppCredentials>("get_app_credentials", { workspaceId, appId }),
  saveAppDetails: (workspaceId: string, value: Application, credentials: AppCredentials) =>
    command<Application>("save_app_details", { workspaceId, value, credentials }),
  saveAppExcelTemplate: (path: string) => command<void>("save_app_excel_template", { path }),
  pickAppExcel: () => command<AppExcelSource | null>("pick_app_excel"),
  readAppExcelFile: async (file: File): Promise<AppExcelSource> => {
    if (!native) throw new Error("此功能需要在桌面应用中使用");
    if (file.size > 5 * 1024 * 1024) throw new Error("Excel 文件不能超过 5 MB。");
    const contents = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onerror = () => reject(new Error("无法读取选择的文件。"));
      reader.onload = () => {
        const data = reader.result;
        if (typeof data === "string" && data.includes(",")) {
          resolve(data.slice(data.indexOf(",") + 1));
        } else {
          reject(new Error("无法读取选择的文件。"));
        }
      };
      reader.readAsDataURL(file);
    });
    return { fileName: file.name, contents };
  },
  previewAppExcel: (source: AppExcelSource) =>
    command<AppExcelPreview>("preview_app_excel", { ...source }),
  importAppExcel: (workspaceId: string, source: AppExcelSource) =>
    command<BrowserBookmarkImportResult>("import_app_excel", { workspaceId, ...source }),
  readBrowserBookmarks: () => command<BrowserBookmarks[]>("read_browser_bookmarks"),
  importBrowserBookmarkApps: (workspaceId: string, items: BrowserBookmarkAppInput[]) =>
    command<BrowserBookmarkImportResult>("import_browser_bookmark_apps", { workspaceId, items }),
  listSharedTables: (workspaceId: string): Promise<SharedTable[]> =>
    native ? command<SharedTable[]>("list_shared_tables", { workspaceId }) : Promise.resolve([]),
  saveSharedTable: (workspaceId: string, input: SharedTableInput) =>
    command<SharedTable>("save_shared_table", { workspaceId, input }),
  deleteSharedTable: (workspaceId: string, id: string, revision: number) =>
    command<void>("delete_shared_table", { workspaceId, id, revision }),
  listSharedTableGroups: (workspaceId: string): Promise<SharedTableGroup[]> =>
    native ? command<SharedTableGroup[]>("list_shared_table_groups", { workspaceId }) : Promise.resolve([]),
  saveSharedTableGroup: (workspaceId: string, input: SharedTableGroupInput) =>
    command<SharedTableGroup>("save_shared_table_group", { workspaceId, input }),
  deleteSharedTableGroup: (workspaceId: string, id: string, revision: number) =>
    command<void>("delete_shared_table_group", { workspaceId, id, revision }),
  meetingRealtimeStart: (workspaceId: string, id: string, modelId: string, sessionId: string, onEvent: Channel<MeetingRealtimeEvent>) => command<void>("meeting_realtime_start", {workspaceId, id, modelId, sessionId, onEvent}),
  meetingRealtimePush: (workspaceId: string, id: string, sessionId: string, chunkId: string) => command<void>("meeting_realtime_push", {workspaceId, id, sessionId, chunkId}),
  meetingRealtimeStop: (workspaceId: string, id: string, sessionId: string) => command<Meeting>("meeting_realtime_stop", {workspaceId, id, sessionId}),
  meetingList: (workspaceId: string) => command<Meeting[]>("meeting_list", { workspaceId }),
  meetingSave: (workspaceId: string, value: Meeting) => command<Meeting>("meeting_save", { workspaceId, value }),
  meetingDelete: (workspaceId: string, id: string, revision: number) => command<void>("meeting_delete", { workspaceId, id, revision }),
  meetingAppendAudio: (workspaceId: string, id: string, chunkId: string, audioBase64: string) => command<Meeting>("meeting_append_audio", { workspaceId, id, chunkId, audioBase64 }),
  meetingTranscribe: (workspaceId: string, id: string, modelId: string, chunkId?: string) => command<Meeting>("meeting_transcribe", { workspaceId, id, modelId, chunkId }),
  meetingGenerate: (workspaceId: string, id: string, modelId: string, revision: number) => command<Meeting>("meeting_generate", { workspaceId, id, modelId, revision }),
  meetingActionComplete: (workspaceId: string, id: string, actionId: string, completed: boolean, revision: number) => command<Meeting>("meeting_action_complete", { workspaceId, id, actionId, completed, revision }),
  meetingActionTask: (workspaceId: string, id: string, actionId: string, revision: number) => command<{ meeting: Meeting; task: import("./types").Task }>("meeting_action_task", { workspaceId, id, actionId, revision }),
  meetingAddMarker: (workspaceId: string, id: string, markerId: string, atMs: number, text: string) => command<Meeting>("meeting_add_marker", { workspaceId, id, markerId, atMs, text }),
  meetingAudio: (workspaceId: string, id: string) => command<{ audioBase64: string; mimeType: "audio/wav" }>("meeting_audio", { workspaceId, id }),
  promptList: () => command<PromptEntry[]>("prompt_list"),
  promptSave: (value: PromptEntry) => command<PromptEntry>("prompt_save", { value }),
  promptDelete: (id: string, revision?: number) => command<void>("prompt_delete", { id, revision }),
  promptGenerate: (request: PromptRequest, onProgress: (progress: PromptProgress) => void) => {
    const channel = new Channel<PromptProgress>();
    channel.onmessage = onProgress;
    return command<PromptResult>("prompt_generate", { request, channel });
  },
  promptCancel: (generationId: string) => command<void>("prompt_cancel", { generationId }),
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
  localBugSave: (workspaceId: string, value: BugDraft & { revision?: number; status?: ZentaoBug["status"] }) =>
    command<ZentaoBug>("local_bug_save", { workspaceId, value }),
  zentaoBugCreateOptions: (workspaceId: string, projectId: string) =>
    command<ZentaoBugCreateOptions>("zentao_bug_create_options", { workspaceId, projectId }),
  zentaoBugCreate: (workspaceId: string, value: ZentaoBugDraft) =>
    command<{ remoteId: string; assignedToSelf: boolean }>("zentao_bug_create", { workspaceId, value }),
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
