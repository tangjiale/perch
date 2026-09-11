import { command, native } from "./api";
export interface MailAccount {
  id: string;
  revision?: number;
  name: string;
  email: string;
  senderName: string;
  username: string;
  imapHost: string;
  imapPort: number;
  imapSecurity: "tls" | "starttls";
  smtpHost: string;
  smtpPort: number;
  smtpSecurity: "tls" | "starttls";
  smtpUsername: string;
  enabled: boolean;
  syncIntervalSeconds: number;
  hasImapCredential?: boolean;
  hasSmtpCredential?: boolean;
  lastSync?: number;
  lastError?: string;
}
export interface MailFolder {
  path: string;
  name: string;
  kind: string;
  unread: number;
  total: number;
}
export interface MailMessage {
  id: string;
  accountId: string;
  folder: string;
  uid: number;
  subject: string;
  from: string;
  to: string[];
  cc: string[];
  date: number;
  preview: string;
  seen: boolean;
  flagged: boolean;
  hasAttachments: boolean;
}
export interface MailAttachment {
  id: string;
  name: string;
  size: number;
  contentType: string;
}
export interface MailDetail extends MailMessage {
  text: string;
  html: string;
  attachments: MailAttachment[];
  messageId?: string;
}
export interface MailDraft {
  id: string;
  accountId: string;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  text: string;
  attachmentPaths: string[];
  replyToMessageId?: string;
  updatedAt?: number;
}
export interface StoredMailDraft {
  id: string;
  accountId: string;
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  text: string;
  attachments: string[];
  updatedAt?: number;
}
export function fromStoredDraft(draft: StoredMailDraft): MailDraft {
  return {
    ...draft,
    to: draft.to.join(", "),
    cc: draft.cc.join(", "),
    bcc: draft.bcc.join(", "),
    attachmentPaths: draft.attachments,
  };
}
export function toStoredDraft(draft: MailDraft): StoredMailDraft {
  const split = (s: string) =>
    s
      .split(/[,;，；\n]/)
      .map((v) => v.trim())
      .filter(Boolean);
  return {
    id: draft.id,
    accountId: draft.accountId,
    to: split(draft.to),
    cc: split(draft.cc),
    bcc: split(draft.bcc),
    subject: draft.subject,
    text: draft.text,
    attachments: draft.attachmentPaths,
    updatedAt: draft.updatedAt,
  };
}
export interface MailUnread {
  total: number;
  accounts: { accountId: string; unread: number }[];
}
export const mailKeys = {
  accounts: ["mail", "accounts"],
  folders: ["mail", "folders"],
  unread: ["mail", "unread"],
};
export const mailApi = {
  accounts: () =>
    native ? command<MailAccount[]>("mail_accounts") : Promise.resolve([]),
  folders: (accountId: string) =>
    native
      ? command<MailFolder[]>("mail_folders", { accountId })
      : Promise.resolve([]),
  unread: () =>
    native
      ? command<MailUnread>("mail_unread")
      : Promise.resolve({ total: 0, accounts: [] }),
};
