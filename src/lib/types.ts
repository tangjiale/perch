export type EntityKind =
  | "task"
  | "project"
  | "app"
  | "category"
  | "provider"
  | "model"
  | "agent"
  | "skill"
  | "knowledge"
  | "document"
  | "conversation"
  | "message"
  | "connection";
export interface Base {
  id: string;
  revision?: number;
  createdAt?: number;
  updatedAt?: number;
}
export type Status = "todo" | "doing" | "done" | "closed";
export interface Schedule {
  kind: "timed" | "all_day";
  timezone: string;
  start: string;
  end?: string;
}
export interface Task extends Base {
  title: string;
  notes: string;
  source: "local" | "zentao";
  status: Status;
  priority: "low" | "normal" | "high";
  projectId?: string;
  schedule?: Schedule;
  sortOrder: number;
  remoteType?: string;
  remoteId?: string;
  remoteStatus?: string;
  remoteBegin?: string;
  remoteEnd?: string;
  remoteDescription?: string;
  remoteScheduleManaged?: boolean;
  remoteExecutionOwner?: string;
  remoteExecutionOwnerAccount?: string;
  connectionId?: string;
  completedAt?: number | null;
}
export interface Project extends Base {
  name: string;
  description: string;
  source: "local" | "zentao";
  status: Status;
  owner: string;
  dueDate?: string;
  plannedStartDate?: string;
  plannedEndDate?: string;
  remoteId?: string;
  connectionId?: string;
}
export interface Application extends Base {
  name: string;
  url: string;
  description: string;
  categoryId?: string;
  logo?: string;
  favorite: boolean;
  sortOrder: number;
}
export interface Category extends Base {
  name: string;
}
export interface Provider extends Base {
  name: string;
  baseUrl: string;
  protocol: "openai-completions" | "openai-responses" | "anthropic-messages";
  enabled: boolean;
  hasCredential?: boolean;
}
export interface Model extends Base {
  providerId: string;
  name: string;
  remoteModelId: string;
  capability: "chat" | "vision" | "embedding";
  enabled: boolean;
  dimensions?: number;
}
export interface Agent extends Base {
  name: string;
  description: string;
  systemPrompt: string;
  modelId: string;
  temperature: number;
  maxTokens: number;
  enabled: boolean;
  skillIds: string[];
}
export interface Skill extends Base {
  source?: string;
  sourceGroup?: string;
  sourcePath?: string;
  name: string;
  description: string;
  content: string;
  enabled: boolean;
}
export interface Knowledge extends Base {
  name: string;
  description: string;
  modelId: string;
  activeGenerationId?: string;
  status?: string;
}
export interface Document extends Base {
  knowledgeId: string;
  name: string;
  size: number;
  assetPath?: string;
  sha256?: string;
  text?: string;
  status: string;
  error?: string;
  chunks?: number;
}
export interface Conversation extends Base {
  title: string;
  agentId: string;
  groupId?: string;
  groupName?: string;
  agentSnapshot?: Agent & {
    model?: Model;
    provider?: Provider;
    skills?: Skill[];
  };
  knowledgeId?: string;
}
export interface Citation {
  documentId: string;
  name: string;
  excerpt: string;
  locator?: string;
}
export interface ChatImage {
  name: string;
  dataUrl: string;
}
export interface Message extends Base {
  usage?: { inputTokens?: number; outputTokens?: number; totalTokens?: number };
  durationMs?: number;
  finishedAt?: number;
  conversationId: string;
  role: "user" | "assistant";
  content: string;
  status: string;
  generationId?: string;
  citations?: Citation[];
  images?: ChatImage[];
}
export interface Connection extends Base {
  name: string;
  baseUrl: string;
  apiVersion: "v1" | "v2";
  enabled: boolean;
  managementEnabled: boolean;
  hasCredential?: boolean;
  rememberCredentials?: boolean;
  loginAccount?: string;
  authMode?: "account" | "token";
  lastSync?: number;
  lastBugSync?: number;
  syncIntervalMinutes?: number;
  error?: string;
}
export interface Snapshot {
  bugs?: ZentaoBug[];
  tasks: Task[];
  projects: Project[];
  apps: Application[];
  categories: Category[];
  providers: Provider[];
  models: Model[];
  agents: Agent[];
  skills: Skill[];
  knowledge: Knowledge[];
  documents: Document[];
  conversations: Conversation[];
  messages: Message[];
  connections: Connection[];
}

export interface ZentaoBug extends Base {
  connectionId: string;
  remoteId: string;
  title: string;
  steps: string;
  status: "active" | "resolved" | "closed";
  assignedTo: string;
  assignedToName?: string;
  lastAssignedTo?: string;
  severity: number;
  priority: number;
  productId?: string;
  productName?: string;
  projectId?: string;
  projectName?: string;
  resolution?: string;
  openedDate?: string;
  resolvedDate?: string;
  deadline?: string;
}
export interface StorageInfo {
  dataRoot: string;
  defaultRoot: string;
  workspaceId: string;
  version: string;
}
export interface PageProps {
  data: Snapshot;
  refresh: () => Promise<unknown>;
  notify: (text: string) => void;
}
export const statuses: Record<Status, string> = {
  todo: "待做",
  doing: "正在做",
  done: "已完成",
  closed: "已关闭",
};
