import { useState, useEffect, useRef } from "react";
import { useSearchParams } from "react-router-dom";
import { Channel } from "@tauri-apps/api/core";
import {
  Plus,
  Send,
  Square,
  Copy,
  MessagesSquare,
  Pencil,
  Trash2,
  ChevronDown,
  ChevronRight,
  FolderInput,
  FolderPlus,
  Folder,
  FolderOpen,
  MoreHorizontal,
  Library,
  Bot,
  ImagePlus,
  X,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import GlassSelect from "../components/GlassSelect";
import Modal from "../components/Modal";
import { api, command } from "../lib/api";
import type {
  PageProps,
  Citation,
  ChatImage,
  Conversation,
} from "../lib/types";
import ChatMessageFooter from "../components/ChatMessageFooter";
import { newChatConversation } from "../lib/chat-conversation";
import { readChatImages } from "../lib/chat-images";
import "./chat-images.css";

interface ChatGroup {
  id: string;
  name: string;
  collapsed?: boolean;
}

const CHAT_GROUPS_KEY = "perch-chat-groups";

function loadChatGroups(): ChatGroup[] {
  try {
    const value = JSON.parse(localStorage.getItem(CHAT_GROUPS_KEY) || "[]");
    return Array.isArray(value)
      ? value.filter(
          (item): item is ChatGroup =>
            !!item && typeof item.id === "string" && typeof item.name === "string",
        )
      : [];
  } catch {
    return [];
  }
}

export default function Chat({ data, refresh, notify }: PageProps) {
  const [params, setParams] = useSearchParams();
  const id = params.get("conversation") || data.conversations[0]?.id;
  const [createdConversation, setCreatedConversation] =
    useState<Conversation | null>(null);
  const current =
    data.conversations.find((c) => c.id === id) ||
    (createdConversation?.id === id ? createdConversation : undefined);
  const [creating, setCreating] = useState(false);
  const [groups, setGroups] = useState<ChatGroup[]>(loadChatGroups);
  const [movingConversation, setMovingConversation] = useState<string | null>(null);
  const [selectedGroupId, setSelectedGroupId] = useState<string | undefined>();
  const [ungroupedCollapsed, setUngroupedCollapsed] = useState(false);
  const [newGroupOpen, setNewGroupOpen] = useState(false);
  const [groupNameDraft, setGroupNameDraft] = useState("");
  const [contextGroupId, setContextGroupId] = useState<string | null>(null);
  const [editingGroupId, setEditingGroupId] = useState<string | null>(null);
  const submitting = useRef(false);
  const [draft, setDraft] = useState(""),
    [agent, setAgent] = useState(data.agents.find((a) => a.enabled)?.id || ""),
    [knowledge, setKnowledge] = useState(""),
    [run, setRun] = useState<string | null>(null),
    [stream, setStream] = useState(""),
    [refs, setRefs] = useState<Citation[]>([]);
  const bottom = useRef<HTMLDivElement>(null);
  const activeRun = useRef<string | null>(null);
  const [pendingUser, setPendingUser] = useState("");
  const [images, setImages] = useState<ChatImage[]>([]);
  const [pendingImages, setPendingImages] = useState<ChatImage[]>([]);
  const [readingImages, setReadingImages] = useState(false);
  const imageInput = useRef<HTMLInputElement>(null);
  const activeConversation = useRef(id);
  activeConversation.current = id;
  const currentAgent =
    current?.agentSnapshot ||
    data.agents.find((a) => a.id === (current?.agentId || agent));
  const currentModel = data.models.find((m) => m.id === currentAgent?.modelId);
  const canUseImages =
    currentModel?.capability === "vision" && currentModel.enabled;
  function persistGroups(next: ChatGroup[]) {
    setGroups(next);
    localStorage.setItem(CHAT_GROUPS_KEY, JSON.stringify(next));
  }
  function groupFor(id?: string) {
    return groups.find((group) => group.id === id);
  }
  function createGroup() {
    const name = groupNameDraft.trim();
    if (!name) return;
    if (groups.some((group) => group.name === name)) {
      notify("已有同名项目");
      return;
    }
    persistGroups([...groups, { id: crypto.randomUUID(), name }]);
    setGroupNameDraft("");
    setNewGroupOpen(false);
  }
  function renameGroup(group: ChatGroup) {
    const name = groupNameDraft.trim();
    if (!name || name === group.name) {
      setEditingGroupId(null);
      return;
    }
    if (groups.some((item) => item.id !== group.id && item.name === name)) {
      notify("已有同名项目");
      return;
    }
    persistGroups(groups.map((item) => item.id === group.id ? { ...item, name } : item));
    void Promise.all(
      data.conversations
        .filter((conversation) => conversation.groupId === group.id)
        .map((conversation) => api.save("conversation", { ...conversation, groupName: name })),
    ).then(() => refresh()).catch((error) => notify((error as Error).message));
    setEditingGroupId(null);
    setContextGroupId(null);
    setGroupNameDraft("");
  }
  async function deleteGroup(group: ChatGroup) {
    if (!confirm(`删除项目“${group.name}”？会话将移到未归入项目`)) return;
    try {
      await Promise.all(
        data.conversations
          .filter((conversation) => conversation.groupId === group.id)
          .map((conversation) => api.save("conversation", {
            ...conversation,
            groupId: undefined,
            groupName: undefined,
          })),
      );
      persistGroups(groups.filter((item) => item.id !== group.id));
      await refresh();
    } catch (error) {
      notify((error as Error).message);
    }
  }
  function toggleGroup(group: ChatGroup) {
    persistGroups(groups.map((item) => item.id === group.id ? { ...item, collapsed: !item.collapsed } : item));
  }
  async function moveConversation(conversation: Conversation, group?: ChatGroup) {
    try {
      await api.save("conversation", {
        ...conversation,
        groupId: group?.id,
        groupName: group?.name,
      });
      setMovingConversation(null);
      await refresh();
    } catch (error) {
      notify((error as Error).message);
    }
  }
  async function deleteConversation(conversation: Conversation) {
    if (!confirm(`删除会话“${conversation.title}”及其消息？`)) return;
    try {
      await api.remove("conversation", conversation.id, conversation.revision);
      if (conversation.id === id) setParams({});
      await refresh();
    } catch (error) {
      notify((error as Error).message);
    }
  }
  useEffect(
    () => () => {
      if (activeRun.current)
        void command("chat_cancel", { generationId: activeRun.current }).catch(
          () => {},
        );
    },
    [],
  );
  const [citation, setCitation] = useState<Citation | null>(null);
  useEffect(() => {
    if (activeRun.current) return;
    setKnowledge(current?.knowledgeId || "");
    setStream("");
    setCitation(null);
    setImages([]);
  }, [id]);
  useEffect(() => {
    bottom.current?.scrollIntoView({ behavior: "smooth" });
  }, [stream, id, data.messages.length]);
  const messages = data.messages
    .filter((m) => m.conversationId === id)
    .sort((a, b) => (a.createdAt || 0) - (b.createdAt || 0));
  async function addImages(files: File[]) {
    if (!canUseImages || run || readingImages || !files.length) return;
    const conversationId = id;
    setReadingImages(true);
    try {
      const next = await readChatImages(files, images);
      if (activeConversation.current === conversationId) setImages(next);
    } catch (error) {
      notify((error as Error).message);
    } finally {
      setReadingImages(false);
    }
  }
  async function createConversation() {
    const group = groupFor(selectedGroupId);
    const c = await api.save(
      "conversation",
      {
        ...newChatConversation(data, agent, knowledge, crypto.randomUUID()),
        groupId: group?.id,
        groupName: group?.name,
      },
    );
    setCreatedConversation(c);
    setParams({ conversation: c.id });
    return c;
  }
  async function create() {
    if (submitting.current || run) return;
    submitting.current = true;
    setCreating(true);
    try {
      await createConversation();
      await refresh();
    } catch (e) {
      notify((e as Error).message);
    } finally {
      submitting.current = false;
      setCreating(false);
    }
  }
  async function send(e: React.FormEvent) {
    e.preventDefault();
    if (
      (!draft.trim() && !images.length) ||
      run ||
      readingImages ||
      submitting.current
    )
      return;
    if (images.length && !canUseImages) {
      notify("当前模型不支持图片，请选择使用视觉模型的 Agent 并新建会话");
      return;
    }
    submitting.current = true;
    const text = draft;
    const sentImages = images;
    const generationId = crypto.randomUUID();
    activeRun.current = generationId;
    setRun(generationId);
    setPendingUser(text);
    setPendingImages(sentImages);
    setImages([]);
    setDraft("");
    setStream("");
    setRefs([]);
    const channel = new Channel<{
      generationId: string;
      content?: string;
      error?: string;
      citations?: Citation[];
    }>();
    channel.onmessage = (event) => {
      if (event.generationId !== generationId) return;
      if (event.content !== undefined) setStream(event.content);
      if (event.citations) setRefs(event.citations);
    };
    try {
      const conversation = current || (await createConversation());
      const saved = await api.save("conversation", {
        ...conversation,
        title:
          conversation.title === "新会话"
            ? text.trim().slice(0, 24) || "图片对话"
            : conversation.title,
        knowledgeId: knowledge || undefined,
      });
      setCreatedConversation(saved);
      await command("chat_send", {
        conversationId: saved.id,
        content: text,
        images: sentImages,
        generationId,
        knowledgeId: knowledge || null,
        channel,
      });
    } catch (e) {
      setDraft(text);
      setImages(sentImages);
      notify((e as Error).message);
    } finally {
      activeRun.current = null;
      try {
        await refresh();
      } catch (error) {
        notify(`会话刷新失败：${(error as Error).message}`);
      } finally {
        submitting.current = false;
        setRun(null);
        setStream("");
        setPendingUser("");
        setPendingImages([]);
      }
    }
  }
  const sortedConversations = data.conversations
    .slice()
    .sort((a, b) => (b.updatedAt || 0) - (a.updatedAt || 0));
  function renderConversation(conversation: Conversation) {
    return (
      <div className="conversation-row" key={conversation.id}>
        <button
          type="button"
          disabled={!!run || creating}
          className={conversation.id === id ? "active" : ""}
          onClick={() => setParams({ conversation: conversation.id })}
        >
          <MessagesSquare size={15} />
          <span
            ref={(element) => {
              if (element && element.scrollWidth > element.clientWidth)
                element.dataset.overflow = "true";
            }}
          >
            {conversation.title}
          </span>
        </button>
        <button
          type="button"
          className="conversation-move"
          disabled={!!run || creating}
          title="移动到项目"
          aria-label={`移动会话“${conversation.title}”到项目`}
          onClick={() => setMovingConversation((value) => value === conversation.id ? null : conversation.id)}
        >
          <FolderInput size={14} />
        </button>
        <button
          type="button"
          className="conversation-delete"
          disabled={!!run || creating}
          title="删除会话"
          aria-label={`删除会话“${conversation.title}”`}
          onClick={() => void deleteConversation(conversation)}
        >
          <Trash2 size={14} />
        </button>
        {movingConversation === conversation.id && (
          <div className="conversation-move-menu" role="menu">
            <button type="button" role="menuitem" onClick={() => void moveConversation(conversation)}>
              未归入项目
            </button>
            {groups.map((group) => (
              <button type="button" key={group.id} role="menuitem" onClick={() => void moveConversation(conversation, group)}>
                {group.name}
              </button>
            ))}
          </div>
        )}
      </div>
    );
  }
  return (
    <div className="chat-layout">
      <aside className="chat-history">
        <h2>会话</h2>
        <GlassSelect
          aria-label="选择 Agent"
          icon={<Bot size={17} />}
          value={agent}
          disabled={!!run || creating}
          onValueChange={setAgent}
          options={[
            { value: "", label: "选择 Agent" },
            ...data.agents
              .filter((a) => a.enabled)
              .map((a) => ({ value: a.id, label: a.name })),
          ]}
        />
        {groups.length > 0 && (
          <GlassSelect
            aria-label="新会话项目"
            icon={<FolderInput size={16} />}
            value={selectedGroupId || ""}
            disabled={!!run || creating}
            onValueChange={(value) => setSelectedGroupId(value || undefined)}
            options={[
              { value: "", label: "新会话不归入项目" },
              ...groups.map((group) => ({ value: group.id, label: group.name })),
            ]}
          />
        )}
        <button type="button" onClick={() => void create()} disabled={!!run || creating}>
          <Plus size={15} />
          新会话
        </button>
        <div className="conversation-groups-toolbar">
          <span>会话记录 / 项目</span>
          <button type="button" className="icon-button" title="新建项目" aria-label="新建项目" onClick={() => { setNewGroupOpen(true); setGroupNameDraft(""); }}>
            <FolderPlus size={15} />
          </button>
        </div>
        {newGroupOpen && (
          <Modal title="创建项目" onClose={() => setNewGroupOpen(false)}>
            <form className="chat-group-modal-form" onSubmit={(event) => { event.preventDefault(); createGroup(); }}>
              <label htmlFor="chat-project-name">项目名称</label>
              <input id="chat-project-name" autoFocus value={groupNameDraft} placeholder="输入项目名称" onChange={(event) => setGroupNameDraft(event.target.value)} />
              <div className="chat-group-modal-actions">
                <button type="button" onClick={() => setNewGroupOpen(false)}>取消</button>
                <button type="submit" className="primary" disabled={!groupNameDraft.trim()}>创建项目</button>
              </div>
            </form>
          </Modal>
        )}
        <div className="conversation-list">
          {groups.map((group) => {
            const conversations = sortedConversations.filter((conversation) => conversation.groupId === group.id);
            return (
              <section className="conversation-group" key={group.id}>
                <header onContextMenu={(event) => { event.preventDefault(); setContextGroupId(group.id); }}>
                  {editingGroupId === group.id ? (
                    <input
                      className="conversation-group-name-input"
                      autoFocus
                      value={groupNameDraft}
                      onChange={(event) => setGroupNameDraft(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") renameGroup(group);
                        if (event.key === "Escape") setEditingGroupId(null);
                      }}
                      onBlur={() => renameGroup(group)}
                    />
                  ) : (
                  <button type="button" className="conversation-group-toggle" onClick={() => toggleGroup(group)}>
                    {group.collapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
                    {group.collapsed ? <Folder size={15} /> : <FolderOpen size={15} />}
                    <span>{group.name}</span>
                    <small>{conversations.length}</small>
                  </button>
                  )}
                  <div className="conversation-group-actions">
                    <button
                      type="button"
                      className="icon-button project-more-button"
                      title="项目操作"
                      aria-label={`项目操作：${group.name}`}
                      aria-expanded={contextGroupId === group.id}
                      onClick={(event) => {
                        event.stopPropagation();
                        setContextGroupId((value) => value === group.id ? null : group.id);
                      }}
                    >
                      <MoreHorizontal size={16} />
                    </button>
                  </div>
                  {contextGroupId === group.id && editingGroupId !== group.id && (
                    <div className="conversation-group-context" role="menu">
                      <button type="button" onClick={() => { setEditingGroupId(group.id); setGroupNameDraft(group.name); setContextGroupId(null); }}>重命名项目</button>
                      <button type="button" onClick={() => { setContextGroupId(null); void deleteGroup(group); }}>删除项目</button>
                    </div>
                  )}
                </header>
                {!group.collapsed && (
                  conversations.length ? conversations.map(renderConversation) : (
                    <p className="conversation-group-empty">暂无会话</p>
                  )
                )}
              </section>
            );
          })}
          <section className="conversation-group">
            <header>
              <button
                type="button"
                className="conversation-group-toggle"
                onClick={() => setUngroupedCollapsed((value) => !value)}
                aria-expanded={!ungroupedCollapsed}
              >
                {ungroupedCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
                {ungroupedCollapsed ? <Folder size={15} /> : <FolderOpen size={15} />}
                <span>未归入项目</span>
                <small>{sortedConversations.filter((conversation) => !conversation.groupId).length}</small>
              </button>
            </header>
            {!ungroupedCollapsed && (
              sortedConversations.filter((conversation) => !conversation.groupId).length
                ? sortedConversations.filter((conversation) => !conversation.groupId).map(renderConversation)
                : <p className="conversation-group-empty">暂无会话</p>
            )}
          </section>
        </div>
      </aside>
      <section className="chat-main">
        <header>
          <div>
            <strong>
              {current?.agentSnapshot?.name ||
                data.agents.find((a) => a.id === current?.agentId)?.name ||
                "聊天"}
            </strong>
            <small>{current?.title || "选择 Agent 并创建会话"}</small>
          </div>
          {current && (
            <div className="actions">
              <button
                className="icon-button"
                disabled={!!run || creating}
                title="重命名"
                onClick={async () => {
                  const title = prompt("会话名称", current.title);
                  if (title)
                    try {
                      await api.save("conversation", { ...current, title });
                      await refresh();
                    } catch (e) {
                      notify((e as Error).message);
                    }
                }}
              >
                <Pencil size={15} />
              </button>
              <button
                className="icon-button"
                disabled={!!run || creating}
                title="删除会话"
                onClick={async () => {
                  if (confirm("删除会话及消息？"))
                    try {
                      await api.remove(
                        "conversation",
                        current.id,
                        current.revision,
                      );
                      await refresh();
                      setParams({});
                    } catch (e) {
                      notify((e as Error).message);
                    }
                }}
              >
                <Trash2 size={15} />
              </button>
            </div>
          )}
        </header>
        <div className="messages">
          {messages.map((m) => (
            <article
              key={m.id}
              className={"message " + m.role}
              aria-label={m.role === "user" ? "你的消息" : "助手消息"}
            >
              <div className="message-heading">
                {m.role !== "user" && (
                  <strong>{current?.agentSnapshot?.name || "助手"}</strong>
                )}
                {m.role === "user" && (
                  <button
                    className="icon-button"
                    title="复制消息"
                    onClick={() =>
                      navigator.clipboard
                        .writeText(m.content)
                        .catch((e) => notify(String(e)))
                    }
                  >
                    <Copy size={13} />
                  </button>
                )}
              </div>
              <ImageGallery images={m.images || []} />
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                components={{ img: () => null }}
              >
                {m.content}
              </ReactMarkdown>
              {!["completed", "pending"].includes(m.status) && (
                <small className="muted">
                  {{
                    stopped: "已停止",
                    failed: "生成失败",
                    streaming: "生成中",
                    interrupted: "已中断",
                  }[m.status] || m.status}
                </small>
              )}
              {m.citations?.length ? (
                <div className="citations">
                  {m.citations.map((c, i) => (
                    <button key={i} onClick={() => setCitation(c)}>
                      [{i + 1}] {c.name}
                    </button>
                  ))}
                </div>
              ) : null}
              {m.role === "assistant" && (
                <ChatMessageFooter message={m} notify={notify} />
              )}
            </article>
          ))}
          {run &&
            (!!pendingUser || !!pendingImages.length) &&
            !messages.some(
              (m) => m.role === "user" && m.generationId === run,
            ) && (
              <article className="message user" aria-label="你的消息">
                <ImageGallery images={pendingImages} />
                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                  {pendingUser}
                </ReactMarkdown>
              </article>
            )}
          {run && (
            <article className="message assistant">
              <strong>正在生成…</strong>
              <ReactMarkdown remarkPlugins={[remarkGfm]}>
                {stream}
              </ReactMarkdown>
              {!!refs.length && (
                <div className="citations">
                  {refs.map((c, i) => (
                    <button key={i} onClick={() => setCitation(c)}>
                      [{i + 1}] {c.name}
                    </button>
                  ))}
                </div>
              )}
            </article>
          )}
          {!current && (
            <div className="empty">
              <MessagesSquare />
              <p>开启一段新的工作对话</p>
            </div>
          )}
          <div ref={bottom} />
        </div>
        {citation && (
          <div className="citation-preview">
            <button onClick={() => setCitation(null)}>关闭</button>
            <strong>{citation.name}</strong>
            <p>{citation.excerpt}</p>
          </div>
        )}
        <form className="chat-composer" onSubmit={send}>
          <textarea
            aria-label="消息"
            disabled={!!run || creating}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder={
              !current
                ? "输入消息，发送时自动创建会话…"
                : canUseImages
                  ? "输入消息，或添加图片让模型分析…"
                  : "输入消息…"
            }
            onPaste={(e) => {
              const files = Array.from(e.clipboardData.files);
              if (files.length && canUseImages) {
                e.preventDefault();
                void addImages(files);
              }
            }}
            onKeyDown={(e) => {
              if (
                e.key === "Enter" &&
                !e.shiftKey &&
                !e.nativeEvent.isComposing
              ) {
                e.preventDefault();
                e.currentTarget.form?.requestSubmit();
              }
            }}
          />
          <ImageGallery
            images={images}
            onRemove={(index) =>
              setImages((current) => current.filter((_, i) => i !== index))
            }
          />
          {!!images.length && !!knowledge && !draft.trim() && (
            <small className="chat-image-hint">
              添加文字问题后才能检索知识库；仅发送图片将直接由视觉模型分析。
            </small>
          )}
          <div className="composer-bottom">
            <div className="chat-image-tools">
              <input
                ref={imageInput}
                type="file"
                hidden
                multiple
                accept="image/png,image/jpeg,image/webp,image/gif"
                aria-label="选择聊天图片"
                onChange={(e) => {
                  void addImages(Array.from(e.target.files || []));
                  e.target.value = "";
                }}
              />
              <button
                type="button"
                disabled={
                  !canUseImages ||
                  !!run ||
                  creating ||
                  readingImages ||
                  images.length >= 4
                }
                onClick={() => imageInput.current?.click()}
                title={
                  canUseImages
                    ? "PNG、JPEG、WebP、GIF；最多 4 张，单张 5 MB，总计 12 MB"
                    : "选择使用视觉模型的 Agent 后可发送图片"
                }
              >
                <ImagePlus size={16} />
                {readingImages ? "读取中…" : "添加图片"}
              </button>
              <small>
                {currentModel?.name ||
                  current?.agentSnapshot?.model?.name ||
                  ""}
                {current && (canUseImages ? " · 视觉" : " · 文本")}
              </small>
              <GlassSelect
                className="chat-knowledge-select"
                aria-label="选择知识库"
                icon={<Library size={16} />}
                disabled={!!run || creating}
                value={knowledge}
                onValueChange={setKnowledge}
                options={[
                  { value: "", label: "不使用知识库" },
                  ...data.knowledge.map((k) => ({
                    value: k.id,
                    label: k.name,
                  })),
                ]}
              />
            </div>
            {run ? (
              <button
                type="button"
                onClick={() =>
                  command("chat_cancel", { generationId: run }).catch((e) =>
                    notify((e as Error).message),
                  )
                }
              >
                <Square size={15} />
                停止
              </button>
            ) : (
              <button
                type="submit"
                className="primary"
                disabled={
                  (!current && !agent) ||
                  (!draft.trim() && !images.length) ||
                  readingImages ||
                  creating
                }
              >
                <Send size={16} />
                发送
              </button>
            )}
          </div>
        </form>
      </section>
    </div>
  );
}

function ImageGallery({
  images,
  onRemove,
}: {
  images: ChatImage[];
  onRemove?: (index: number) => void;
}) {
  if (!images.length) return null;
  return (
    <div className="chat-image-gallery">
      {images.map((image, index) => (
        <figure key={`${index}-${image.name}`}>
          <img
            src={
              /^data:image\/(png|jpeg|webp|gif);base64,/.test(image.dataUrl)
                ? image.dataUrl
                : undefined
            }
            alt={image.name}
          />
          <figcaption title={image.name}>{image.name}</figcaption>
          {onRemove && (
            <button
              type="button"
              className="icon-button"
              aria-label={`移除图片 ${image.name}`}
              onClick={() => onRemove(index)}
            >
              <X size={14} />
            </button>
          )}
        </figure>
      ))}
    </div>
  );
}
