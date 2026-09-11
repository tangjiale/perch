import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { save } from "@tauri-apps/plugin-dialog";
import {
  Archive,
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  FilePenLine,
  Folder,
  Inbox,
  Mail as MailIcon,
  MailOpen,
  Paperclip,
  PencilLine,
  RefreshCw,
  Reply,
  ReplyAll,
  Search,
  Send,
  Settings2,
  ShieldCheck,
  Star,
  Trash2,
  Forward,
} from "lucide-react";
import { command, native } from "../lib/api";
import {
  fromStoredDraft,
  mailApi,
  mailKeys,
  type MailAccount,
  type MailDetail,
  type MailDraft,
  type MailFolder,
  type MailMessage,
  type StoredMailDraft,
} from "../lib/mail";
import { mailHtmlDocument, mailReplyRecipients } from "../lib/mail-content";
import MailComposer from "../components/MailComposer";
import GlassSelect from "../components/GlassSelect";
import "./mail.css";

const kindNames: Record<string, string> = {
  inbox: "收件箱",
  sent: "已发送",
  drafts: "服务器草稿",
  trash: "已删除",
  junk: "垃圾邮件",
  archive: "归档",
};
const kindIcons = {
  inbox: Inbox,
  sent: Send,
  drafts: FilePenLine,
  trash: Trash2,
  junk: ShieldCheck,
  archive: Archive,
};
function folderTitle(folder: MailFolder) {
  return kindNames[folder.kind] || folder.name;
}
function dateText(date: number) {
  return date
    ? new Date(date).toLocaleString("zh-CN", {
        month: "numeric",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "";
}
function newDraft(accountId: string): MailDraft {
  return {
    id: crypto.randomUUID(),
    accountId,
    to: "",
    cc: "",
    bcc: "",
    subject: "",
    text: "",
    attachmentPaths: [],
  };
}

export default function Mail({
  notify,
}: {
  notify: (message: string) => void;
}) {
  const client = useQueryClient();
  const { data: accounts = [], error: accountError } = useQuery({
    queryKey: mailKeys.accounts,
    queryFn: mailApi.accounts,
  });
  const { data: unread } = useQuery({
    queryKey: mailKeys.unread,
    queryFn: mailApi.unread,
  });
  const [accountChoice, setAccountChoice] = useState("");
  const account =
    accounts.find((a) => a.id === accountChoice) ??
    accounts.find((a) => a.enabled) ??
    accounts[0];
  const accountId = account?.id ?? "";
  const [folder, setFolder] = useState("INBOX");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [selectedId, setSelectedId] = useState("");
  const [compose, setCompose] = useState<MailDraft | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [limit, setLimit] = useState(50);
  const [htmlView, setHtmlView] = useState(false);
  const inFlight = useRef(false);
  const pendingRead = useRef("");
  const { data: folders = [], error: foldersError } = useQuery({
    queryKey: [...mailKeys.folders, accountId],
    queryFn: () => mailApi.folders(accountId),
    enabled: !!accountId,
  });
  const localDrafts = folder === "__local_drafts__";
  const deliveries = folder === "__deliveries__";
  const {
    data: deliveryRows = [],
    error: deliveryError,
    isLoading: deliveryLoading,
  } = useQuery({
    queryKey: ["mail", "delivery", accountId],
    queryFn: () =>
      command<(StoredMailDraft & { deliveryState: string })[]>(
        "mail_delivery_history",
        { accountId },
      ),
    enabled: native && !!accountId && deliveries,
  });
  const deliveryLabel = (state: string) =>
    state === "sent"
      ? "已发送"
      : state === "sending"
        ? "发送中 / 待核对"
        : "结果未确认";
  const visibleDeliveries = deliveryRows.filter((row) =>
    `${row.subject} ${row.to.join(" ")} ${row.text}`
      .toLowerCase()
      .includes(query.toLowerCase()),
  );
  const selectedDelivery = deliveries
    ? deliveryRows.find((row) => row.id === selectedId)
    : undefined;
  const {
    data: list = { messages: [], total: 0 },
    error: listError,
    isLoading,
  } = useQuery({
    queryKey: ["mail", "messages", accountId, folder, query, limit],
    queryFn: () =>
      command<{ messages: MailMessage[]; total: number }>("mail_messages", {
        accountId,
        folder,
        search: query,
        limit,
        offset: 0,
      }),
    enabled: native && !!accountId && !localDrafts && !deliveries,
  });
  const { data: drafts = [], error: draftsError } = useQuery({
    queryKey: ["mail", "drafts", accountId],
    queryFn: () => command<StoredMailDraft[]>("mail_drafts", { accountId }),
    enabled: native && !!accountId && localDrafts,
  });
  const {
    data: detail,
    error: detailError,
    isLoading: detailLoading,
  } = useQuery({
    queryKey: ["mail", "message", selectedId],
    queryFn: () =>
      command<MailDetail>("mail_message", { messageId: selectedId }),
    enabled: native && !!selectedId && !deliveries,
  });
  const visible = list.messages.filter((m) =>
    filter === "unread" ? !m.seen : filter === "starred" ? m.flagged : true,
  );
  const theme = document.documentElement.dataset.theme === "dark";
  const htmlDocument = useMemo(
    () => mailHtmlDocument(detail?.html ?? "", theme),
    [detail?.html, theme],
  );
  async function refresh(message?: string) {
    await client.invalidateQueries({ queryKey: ["mail"] });
    if (message) notify(message);
  }
  async function run(operation: () => Promise<unknown>) {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError("");
    try {
      const result = await operation();
      await refresh();
      if (typeof result === "string") notify(result);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }
  async function sync(selectedAccount = accountId, selectedFolder = folder) {
    if (selectedFolder === "__deliveries__") {
      await refresh();
      return;
    }
    await run(() =>
      command("mail_sync", {
        accountId: selectedAccount,
        folder: selectedFolder === "__local_drafts__" ? null : selectedFolder,
      }),
    );
  }
  function choose(id: string, path: string) {
    setAccountChoice(id);
    setFolder(path);
    setSelectedId("");
    setLimit(50);
    setFilter("all");
    setError("");
  }
  // 切换到服务器文件夹后更新缓存；不依赖页面停留来维持后台收信。
  useEffect(() => {
    if (native && accountId && !localDrafts && !deliveries && account?.enabled)
      void sync(accountId, folder);
  }, [accountId, folder]);
  useEffect(() => {
    if (busy || !pendingRead.current) return;
    const id = pendingRead.current;
    pendingRead.current = "";
    if (id === selectedId)
      void run(() =>
        command("mail_message_flag", {
          messageId: id,
          seen: true,
          flagged: null,
        }),
      );
  }, [busy, selectedId]);
  function view(message: MailMessage) {
    setSelectedId(message.id);
    setHtmlView(false);
    pendingRead.current = !message.seen && inFlight.current ? message.id : "";
    if (!message.seen && !inFlight.current)
      void run(() =>
        command("mail_message_flag", {
          messageId: message.id,
          seen: true,
          flagged: null,
        }),
      );
  }
  function reply(mode: "reply" | "all" | "forward") {
    if (!detail || !account) return;
    const to =
      mode === "forward"
        ? ""
        : mode === "all"
          ? mailReplyRecipients(
              detail.to.join(","),
              detail.cc.join(","),
              detail.from,
              account.email,
            )
          : mailReplyRecipients("", "", detail.from, account.email);
    const prefix = mode === "forward" ? "Fwd: " : "Re: ";
    setCompose({
      ...newDraft(account.id),
      to,
      subject: /^(re|fwd):/i.test(detail.subject)
        ? detail.subject
        : prefix + detail.subject,
      text: `\n\n—— ${mode === "forward" ? "转发邮件" : "原邮件"} ——\n发件人：${detail.from}\n时间：${new Date(detail.date).toLocaleString("zh-CN")}\n主题：${detail.subject}\n\n${detail.text || "（原邮件为 HTML，请在阅读区查看）"}`,
    });
  }
  const activeFolder = folders.find((f) => f.path === folder);
  const problem =
    error ||
    accountError?.message ||
    foldersError?.message ||
    listError?.message ||
    detailError?.message ||
    draftsError?.message;
  return (
    <div className="mail-page-layout">
      <aside className="mail-folders-panel">
        <header>
          <h1>邮件</h1>
          <Link
            to="/settings?tab=mail"
            className="icon-button"
            title="邮箱设置"
          >
            <Settings2 size={17} />
          </Link>
        </header>
        <button
          className="primary mail-write"
          disabled={!accounts.some((a) => a.enabled)}
          onClick={() =>
            setCompose(
              newDraft(
                account?.enabled
                  ? accountId
                  : accounts.find((a) => a.enabled)!.id,
              ),
            )
          }
        >
          <PencilLine size={17} />
          写邮件
        </button>
        <div className="mail-account-tree">
          {accounts.map((a) => (
            <div className="mail-tree-account" key={a.id}>
              <button
                disabled={busy}
                className={`mail-tree-account-title ${accountId === a.id ? "selected" : ""}`}
                onClick={() => {
                  if (a.id !== accountId) choose(a.id, "INBOX");
                  setCollapsed((current) => {
                    const next = new Set(current);
                    if (next.has(a.id)) next.delete(a.id);
                    else if (a.id === accountId) next.add(a.id);
                    return next;
                  });
                }}
                aria-expanded={!collapsed.has(a.id)}
              >
                {collapsed.has(a.id) ? (
                  <ChevronRight size={13} />
                ) : (
                  <ChevronDown size={13} />
                )}
                <span className="mail-tree-avatar">{a.name.slice(0, 1)}</span>
                <span
                  className="mail-tree-name"
                  title={`${a.name} · ${a.email}`}
                >
                  {a.name}
                  <small>{a.email}</small>
                </span>
                {(unread?.accounts.find((u) => u.accountId === a.id)?.unread ??
                  0) > 0 && (
                  <span className="mail-count">
                    {unread?.accounts.find((u) => u.accountId === a.id)?.unread}
                  </span>
                )}
              </button>
              {!collapsed.has(a.id) && (
                <div className="mail-tree-folders">
                  {a.id === accountId ? (
                    <>
                      {(folders.length
                        ? folders
                        : [
                            {
                              path: "INBOX",
                              name: "收件箱",
                              kind: "inbox",
                              unread: 0,
                              total: 0,
                            },
                          ]
                      ).map((f) => {
                        const Icon =
                          kindIcons[f.kind as keyof typeof kindIcons] || Folder;
                        return (
                          <button
                            key={f.path}
                            className={folder === f.path ? "active" : ""}
                            disabled={busy}
                            onClick={() => choose(a.id, f.path)}
                          >
                            <Icon size={16} />
                            <span>{folderTitle(f)}</span>
                            {f.unread > 0 && <small>{f.unread}</small>}
                          </button>
                        );
                      })}
                      <button
                        className={localDrafts ? "active" : ""}
                        disabled={busy}
                        onClick={() => choose(a.id, "__local_drafts__")}
                      >
                        <FilePenLine size={16} />
                        <span>本地草稿</span>
                      </button>
                      <button
                        className={deliveries ? "active" : ""}
                        disabled={busy}
                        onClick={() => choose(a.id, "__deliveries__")}
                      >
                        <Send size={16} />
                        <span>
                          {folders.some((f) => f.kind === "sent")
                            ? "本机已发送"
                            : "已发送"}
                        </span>
                      </button>
                    </>
                  ) : (
                    <button
                      disabled={busy}
                      onClick={() => choose(a.id, "INBOX")}
                    >
                      <Inbox size={16} />
                      <span>收件箱</span>
                    </button>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
        <footer>
          <span className={`mail-sync-indicator ${busy ? "syncing" : ""}`} />
          {busy
            ? "正在同步…"
            : account?.lastSync
              ? `上次同步 ${dateText(account.lastSync)}`
              : "等待同步"}
          <Link to="/settings?tab=mail">管理邮箱</Link>
        </footer>
      </aside>
      {!accounts.length ? (
        <div className="mail-onboarding mail-first-use">
          <MailIcon size={40} />
          <h2>工作与生活，收在一处</h2>
          <p>添加邮箱后，在这里阅读、回复和发送邮件。</p>
          <Link className="primary" to="/settings?tab=mail">
            <PlusIcon />
            添加邮箱
          </Link>
          {problem && (
            <p className="error" role="alert">
              {problem}
            </p>
          )}
        </div>
      ) : (
        <>
          <section
            className={`mail-list-panel ${selectedId ? "has-selection" : ""}`}
          >
            <header>
              <div>
                <h2>
                  {deliveries
                    ? "本机已发送"
                    : localDrafts
                      ? "本地草稿"
                      : activeFolder
                        ? folderTitle(activeFolder)
                        : "收件箱"}
                </h2>
                <small>
                  {deliveries
                    ? `${deliveryRows.length} 封发送记录`
                    : localDrafts
                      ? `${drafts.length} 封草稿`
                      : `已缓存 ${list.total} 封`}
                </small>
              </div>
              <button
                className="icon-button"
                disabled={busy || !account?.enabled}
                title="刷新邮件"
                onClick={() => void sync()}
              >
                <RefreshCw size={17} />
              </button>
            </header>
            <label className="mail-search">
              <Search size={15} />
              <input
                placeholder="搜索当前文件夹"
                aria-label="搜索邮件"
                value={query}
                onChange={(e) => {
                  setQuery(e.target.value);
                  setLimit(50);
                }}
              />
            </label>
            {!localDrafts && !deliveries && (
              <div className="mail-list-filters">
                {[
                  ["all", "全部"],
                  ["unread", "未读"],
                  ["starred", "星标"],
                ].map(([value, label]) => (
                  <button
                    key={value}
                    className={filter === value ? "active" : ""}
                    onClick={() => setFilter(value)}
                  >
                    {label}
                  </button>
                ))}
              </div>
            )}
            {(problem || (deliveries && deliveryError)) && (
              <div className="mail-inline-error" role="alert">
                {deliveries && deliveryError ? deliveryError.message : problem}
                <button disabled={busy} onClick={() => void sync()}>
                  重试同步
                </button>
              </div>
            )}
            {account?.lastError && !problem && (
              <div className="mail-inline-error" role="alert">
                {account.lastError}
              </div>
            )}
            <div className="mail-message-list">
              {deliveries
                ? visibleDeliveries.map((row) => (
                    <button
                      key={row.id}
                      className={`mail-message-item ${row.id === selectedId ? "active" : ""}`}
                      onClick={() => setSelectedId(row.id)}
                    >
                      <span className="mail-message-avatar">
                        <Send size={17} />
                      </span>
                      <span className="mail-message-summary">
                        <span className="mail-message-line">
                          <strong>{row.to.join(", ") || "未填写收件人"}</strong>
                          <time>{dateText(row.updatedAt || 0)}</time>
                        </span>
                        <span className="mail-message-subject">
                          {row.subject || "（无主题）"}
                        </span>
                        <span className="mail-message-preview">
                          {row.text || "（邮件没有正文）"}
                        </span>
                        <small>{deliveryLabel(row.deliveryState)}</small>
                      </span>
                    </button>
                  ))
                : localDrafts
                  ? drafts
                      .filter((d) =>
                        `${d.subject} ${d.text}`
                          .toLowerCase()
                          .includes(query.toLowerCase()),
                      )
                      .map((d) => (
                        <div className="mail-draft-row" key={d.id}>
                          <button
                            onClick={() => setCompose(fromStoredDraft(d))}
                          >
                            <strong>{d.subject || "（无主题）"}</strong>
                            <span>{d.to.join(", ") || "未填写收件人"}</span>
                            <small>{d.text.slice(0, 100)}</small>
                          </button>
                          <button
                            className="icon-button"
                            title="删除本地草稿"
                            onClick={() => {
                              if (confirm("删除此本地草稿？"))
                                void run(() =>
                                  command("mail_draft_delete", {
                                    draftId: d.id,
                                  }),
                                );
                            }}
                          >
                            <Trash2 size={14} />
                          </button>
                        </div>
                      ))
                  : visible.map((m) => (
                      <button
                        key={m.id}
                        className={`mail-message-item ${m.id === selectedId ? "active" : ""} ${m.seen ? "" : "unread"}`}
                        onClick={() => view(m)}
                      >
                        <span className="mail-message-avatar">
                          {m.from
                            .replace(/["<]/g, "")
                            .slice(0, 1)
                            .toUpperCase() || "M"}
                        </span>
                        <span className="mail-message-summary">
                          <span className="mail-message-line">
                            <strong>{m.from || "未知发件人"}</strong>
                            <time>{dateText(m.date)}</time>
                          </span>
                          <span className="mail-message-subject">
                            {!m.seen && <i className="mail-unread-dot" />}
                            {m.subject || "（无主题）"}
                          </span>
                          <span className="mail-message-preview">
                            {m.preview || "暂无纯文本摘要"}
                          </span>
                        </span>
                        <span className="mail-message-icons">
                          {m.flagged && <Star size={12} fill="currentColor" />}
                          {m.hasAttachments && <Paperclip size={12} />}
                        </span>
                      </button>
                    ))}
              {(deliveries ? deliveryLoading : isLoading) && (
                <p className="mail-empty" role="status">
                  正在读取邮件…
                </p>
              )}
              {!(deliveries ? deliveryLoading : isLoading) &&
                (deliveries
                  ? !visibleDeliveries.length
                  : localDrafts
                    ? !drafts.length
                    : !visible.length) && (
                  <div className="mail-empty">
                    <Inbox size={28} />
                    <p>
                      {localDrafts
                        ? "还没有保存的草稿"
                        : query || filter !== "all"
                          ? "没有符合条件的邮件"
                          : "这里还没有邮件"}
                    </p>
                    {!localDrafts && !deliveries && (
                      <small>首次连接后点击刷新，读取此文件夹。</small>
                    )}
                  </div>
                )}
              {!localDrafts &&
                !deliveries &&
                list.total > list.messages.length && (
                  <button
                    className="mail-load-more"
                    onClick={() => setLimit((l) => Math.min(200, l + 50))}
                  >
                    加载更多缓存邮件
                  </button>
                )}
            </div>
            {!localDrafts && !deliveries && (
              <p className="mail-cache-note">当前显示最近 200 封的本地缓存</p>
            )}
          </section>
          <section className={`mail-reader ${selectedId ? "selected" : ""}`}>
            {selectedDelivery ? (
              <>
                <div className="mail-reader-toolbar">
                  <button
                    className="icon-button mail-reader-back"
                    title="返回邮件列表"
                    onClick={() => setSelectedId("")}
                  >
                    <ArrowLeft size={17} />
                  </button>
                  <span>{deliveryLabel(selectedDelivery.deliveryState)}</span>
                </div>
                <div className="mail-reader-scroll">
                  <header className="mail-envelope">
                    <h2>{selectedDelivery.subject || "（无主题）"}</h2>
                    <p>
                      <strong>{account?.email}</strong>
                    </p>
                    <p>收件人：{selectedDelivery.to.join(", ")}</p>
                    {!!selectedDelivery.cc.length && (
                      <p>抄送：{selectedDelivery.cc.join(", ")}</p>
                    )}
                    {!!selectedDelivery.bcc.length && (
                      <p>密送：{selectedDelivery.bcc.join(", ")}</p>
                    )}
                    {!!selectedDelivery.updatedAt && (
                      <time>
                        {new Date(selectedDelivery.updatedAt).toLocaleString(
                          "zh-CN",
                        )}
                      </time>
                    )}
                  </header>
                  {!!selectedDelivery.attachments.length && (
                    <div className="mail-attachments">
                      {selectedDelivery.attachments.map((path, index) => (
                        <span key={index}>
                          <Paperclip size={14} />
                          {path.split(/[\\/]/).pop()}
                        </span>
                      ))}
                    </div>
                  )}
                  <p className="mail-cache-note">
                    {selectedDelivery.deliveryState === "sent"
                      ? "SMTP 服务器已接受此邮件，不代表收件人已阅读。"
                      : "发送结果待核对，请先在网页邮箱确认；不会自动重发。"}
                  </p>
                  <div className="mail-text-content">
                    {selectedDelivery.text || "（邮件没有正文）"}
                  </div>
                </div>
              </>
            ) : !deliveries && selectedId && detail ? (
              <>
                <div className="mail-reader-toolbar">
                  <button
                    className="icon-button mail-reader-back"
                    title="返回邮件列表"
                    onClick={() => setSelectedId("")}
                  >
                    <ArrowLeft size={17} />
                  </button>
                  <button
                    className="icon-button"
                    title="回复"
                    disabled={busy}
                    onClick={() => reply("reply")}
                  >
                    <Reply size={18} />
                  </button>
                  <button
                    className="icon-button"
                    title="回复全部"
                    disabled={busy}
                    onClick={() => reply("all")}
                  >
                    <ReplyAll size={18} />
                  </button>
                  <button
                    className="icon-button"
                    title="转发正文"
                    disabled={busy}
                    onClick={() => reply("forward")}
                  >
                    <Forward size={18} />
                  </button>
                  <span className="mail-toolbar-separator" />
                  <button
                    className="icon-button"
                    title={detail.flagged ? "取消星标" : "添加星标"}
                    disabled={busy}
                    onClick={() =>
                      void run(() =>
                        command("mail_message_flag", {
                          messageId: detail.id,
                          seen: null,
                          flagged: !detail.flagged,
                        }),
                      )
                    }
                  >
                    <Star
                      size={17}
                      fill={detail.flagged ? "currentColor" : "none"}
                    />
                  </button>
                  <button
                    className="icon-button"
                    title={detail.seen ? "标为未读" : "标为已读"}
                    disabled={busy}
                    onClick={() =>
                      void run(() =>
                        command("mail_message_flag", {
                          messageId: detail.id,
                          seen: !detail.seen,
                          flagged: null,
                        }),
                      )
                    }
                  >
                    {detail.seen ? (
                      <MailIcon size={17} />
                    ) : (
                      <MailOpen size={17} />
                    )}
                  </button>
                  <GlassSelect
                    aria-label="移动邮件到文件夹"
                    className="mail-move-select"
                    value=""
                    placeholder="移动到…"
                    disabled={busy}
                    options={folders
                      .filter((f) => f.path !== folder)
                      .map((f) => ({ value: f.path, label: folderTitle(f) }))}
                    onValueChange={(targetFolder) => {
                      void run(async () => {
                        await command("mail_message_move", {
                          messageId: detail.id,
                          targetFolder,
                        });
                        setSelectedId("");
                      });
                    }}
                  />
                  {folders.some(
                    (f) => f.kind === "trash" && f.path !== folder,
                  ) && (
                    <button
                      className="icon-button"
                      title="移到已删除"
                      disabled={busy}
                      onClick={() => {
                        void run(async () => {
                          await command("mail_message_move", {
                            messageId: detail.id,
                            targetFolder: folders.find(
                              (f) => f.kind === "trash",
                            )!.path,
                          });
                          setSelectedId("");
                        });
                      }}
                    >
                      <Trash2 size={17} />
                    </button>
                  )}
                </div>
                <div className="mail-reader-scroll">
                  <header className="mail-envelope">
                    <h2>{detail.subject || "（无主题）"}</h2>
                    <p>
                      <strong>{detail.from}</strong>
                    </p>
                    <p>收件人：{detail.to.join(", ")}</p>
                    {detail.cc.length > 0 && (
                      <p>抄送：{detail.cc.join(", ")}</p>
                    )}
                    <time>{new Date(detail.date).toLocaleString("zh-CN")}</time>
                  </header>
                  {detail.attachments.length > 0 && (
                    <div className="mail-attachments">
                      {detail.attachments.map((a) => (
                        <button
                          key={a.id}
                          onClick={() =>
                            void run(async () => {
                              const destination = await save({
                                title: "保存邮件附件",
                                defaultPath: a.name.replace(/[\\/]/g, "_"),
                              });
                              if (destination) {
                                await command("mail_attachment_save", {
                                  messageId: detail.id,
                                  attachmentId: a.id,
                                  destination,
                                });
                                return "附件已保存";
                              }
                            })
                          }
                        >
                          <Paperclip size={14} />
                          <span>
                            {a.name}
                            <small>
                              {Math.max(1, Math.round(a.size / 1024))} KB
                            </small>
                          </span>
                        </button>
                      ))}
                    </div>
                  )}
                  {detail.html && (
                    <div className="mail-content-mode">
                      <button onClick={() => setHtmlView((v) => !v)}>
                        {htmlView ? "显示纯文本" : "显示排版正文"}
                      </button>
                      <span>已拦截外部图片和活动内容</span>
                    </div>
                  )}
                  {htmlView ? (
                    <iframe
                      sandbox=""
                      title="邮件排版正文"
                      className="mail-html-content"
                      srcDoc={htmlDocument}
                    />
                  ) : (
                    <div className="mail-text-content">
                      {detail.text ||
                        (detail.html
                          ? "此邮件仅包含排版正文，请点击上方“显示排版正文”。"
                          : "（邮件没有正文）")}
                    </div>
                  )}
                </div>
              </>
            ) : (
              <div className="mail-reader-empty">
                <MailOpen size={38} />
                <h2>{detailLoading ? "正在读取邮件…" : "选择一封邮件"}</h2>
                <p>在左侧列表中选择邮件，即可在这里阅读。</p>
              </div>
            )}
          </section>
        </>
      )}
      {compose && (
        <MailComposer
          key={compose.id}
          accounts={accounts}
          initial={compose}
          onClose={() => setCompose(null)}
          onSaved={refresh}
        />
      )}
    </div>
  );
}
function PlusIcon() {
  return <PencilLine size={16} />;
}
