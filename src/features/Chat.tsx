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
  Library,
  Bot,
  ImagePlus,
  X,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import GlassSelect from "../components/GlassSelect";
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
export default function Chat({ data, refresh, notify }: PageProps) {
  const [params, setParams] = useSearchParams();
  const id = params.get("conversation") || data.conversations[0]?.id;
  const [createdConversation, setCreatedConversation] =
    useState<Conversation | null>(null);
  const current =
    data.conversations.find((c) => c.id === id) ||
    (createdConversation?.id === id ? createdConversation : undefined);
  const [creating, setCreating] = useState(false);
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
    const c = await api.save(
      "conversation",
      newChatConversation(data, agent, knowledge, crypto.randomUUID()),
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
        <button onClick={() => void create()} disabled={!!run || creating}>
          <Plus size={15} />
          新会话
        </button>
        <div className="conversation-list">
          {data.conversations
            .slice()
            .sort((a, b) => (b.updatedAt || 0) - (a.updatedAt || 0))
            .map((c) => (
              <button
                disabled={!!run || creating}
                className={c.id === id ? "active" : ""}
                key={c.id}
                onClick={() => setParams({ conversation: c.id })}
              >
                <MessagesSquare size={15} />
                <span>{c.title}</span>
              </button>
            ))}
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
