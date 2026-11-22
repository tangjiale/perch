export interface BackupConfig {
  revision: number;
  endpoint: string;
  region: string;
  bucket: string;
  prefix: string;
  forcePathStyle: boolean;
  hasCredentials: boolean;
  enabled: boolean;
  cron: string;
}

export interface BackupSaveInput extends BackupConfig {
  accessKeyId?: string;
  secretAccessKey?: string;
}

export interface BackupRecord {
  id: string;
  key: string;
  filename: string;
  size: number;
  createdAt: string;
  trigger: "manual" | "scheduled" | "remote";
  status: "preparing" | "packing" | "uploading" | "verifying" | "success" | "failed" | "unknown";
  error?: string;
  monthlyArchive: boolean;
  progress?: BackupProgress | null;
  retryAvailable?: boolean;
}

export interface BackupProgress {
  phase: "preparing" | "database" | "packing" | "hashing" | "uploading" | "verifying" | "cleaning" | "completed";
  completed: number;
  total?: number | null;
}

export interface BackupProgressEvent {
  workspaceId: string;
  dataRoot: string;
  record: BackupRecord;
}

export interface BackupState {
  config: BackupConfig;
  records: BackupRecord[];
  nextRunAt?: string | null;
  running: boolean;
}
