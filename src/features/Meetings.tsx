import { Channel } from "@tauri-apps/api/core";
import type { MeetingRealtimeEvent, MeetingSpeechStatus } from "../lib/api";
import { Link, useLocation } from "react-router-dom";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type TextareaHTMLAttributes,
} from "react";
import {
  AudioLines,
  BookmarkPlus,
  Download,
  FileText,
  GitBranch,
  ListChecks,
  Mic,
  Plus,
  Play,
  Save,
  Search,
  Settings2,
  Square,
  Trash2,
  WandSparkles,
} from "lucide-react";
import GlassSelect from "../components/GlassSelect";
import Modal from "../components/Modal";
import MeetingContextFields from "./meetings/MeetingContext";
import MeetingActions from "./meetings/MeetingActions";
import { api, native } from "../lib/api";
import {
  defaultLocalMeetingSpeechModelId,
  eligibleMeetingSpeechModels,
  localMeetingSpeechModels,
  type LocalMeetingSpeechModelId,
  type MeetingSpeechPreferences,
} from "../lib/meeting-speech";
import type { Snapshot } from "../lib/types";
import {
  layoutMindMap,
  defaultMeetingContext,
  makeMeeting,
  meetingMarkdown,
  meetingTime,
  mindMapSvg,
  type Meeting,
  type MeetingAction,
  type MeetingContext,
  type MeetingSegment,
  type MindMapNode,
  speakerLabel,
} from "../lib/meetings";
import { MeetingRecorder } from "../lib/meeting-recorder";
import "./meetings.css";

type RecordingMode = "realtime" | "chunks" | "record";

interface PendingTranscript {
  meetingId: string;
  sessionId: string;
  segments: MeetingSegment[];
}

const speechStatusLabels: Record<MeetingSpeechStatus["state"], string> = {
  ready: "已就绪",
  downloading: "下载中，可先仅录音",
  error: "安装失败，可先仅录音",
  missing: "安装后可实时转录，也可先仅录音",
};

function download(content: BlobPart, type: string, filename: string) {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename.replace(/[\\/:*?"<>|]/g, "_");
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 30000);
}
function fitTranscript(element: HTMLTextAreaElement) {
  if (!element.clientWidth) return;
  // 先恢复自然高度，编辑删字时也能收回空白；边框另计，避免最后一行被裁切。
  element.style.height = "auto";
  element.style.height = `${element.scrollHeight + element.offsetHeight - element.clientHeight}px`;
}
function TranscriptInput(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  const input = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    if (input.current) fitTranscript(input.current);
  }, [props.value]);
  useLayoutEffect(() => {
    const element = input.current;
    if (!element) return;
    let width = element.getBoundingClientRect().width;
    const observer = new ResizeObserver(() => {
      const nextWidth = element.getBoundingClientRect().width;
      // 只响应宽度变化，避免自动增高反复触发自身测量。
      if (width !== nextWidth) {
        width = nextWidth;
        fitTranscript(element);
      }
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return <textarea {...props} ref={input} />;
}
function MindMap({ root }: { root: MindMapNode }) {
  const l = layoutMindMap(root);
  return (
    <div className="meeting-map">
      <svg
        role="img"
        aria-label="会议脑图"
        width={l.width}
        height={l.height}
        viewBox={`0 0 ${l.width} ${l.height}`}
      >
        {l.edges.map((e) => {
          const a = l.nodes[e.from],
            b = l.nodes[e.to];
          return (
            <path
              key={`${e.from}-${e.to}`}
              d={`M${a.x + 190},${a.y + a.height / 2} C${a.x + 210},${a.y + a.height / 2} ${b.x - 20},${b.y + b.height / 2} ${b.x},${b.y + b.height / 2}`}
            />
          );
        })}
        {l.nodes.map((n) => (
          <g key={n.id}>
            <title>{n.label}</title>
            <rect x={n.x} y={n.y} width="190" height={n.height} rx="10" />
            <text className="meeting-map-text">
              {n.lines.map((line, i) => (
                <tspan key={i} x={n.x + 10} y={n.y + 21 + i * 18}>
                  {line}
                </tspan>
              ))}
            </text>
          </g>
        ))}
      </svg>
    </div>
  );
}
export default function Meetings({
  data,
  workspaceId,
  notify,
  onRecordingChange,
  onTasksChanged,
  onOpenTask,
}: {
  data: Snapshot;
  workspaceId: string;
  notify: (text: string) => void;
  onRecordingChange?: (recording: boolean) => void;
  onTasksChanged?: () => Promise<unknown>;
  onOpenTask?: (id: string) => void;
}) {
  const { pathname } = useLocation();
  const [speechStatus, setSpeechStatus] = useState<MeetingSpeechStatus>();
  const [speakerStatus, setSpeakerStatus] = useState<MeetingSpeechStatus>();
  const [speechStatusFailed, setSpeechStatusFailed] = useState(false);
  const [speechPreferences, setSpeechPreferences] = useState<MeetingSpeechPreferences>();
  const [speechPreferencesError, setSpeechPreferencesError] = useState(false);
  const [onlyRecord, setOnlyRecord] = useState(false);
  const [activeMode, setActiveMode] = useState<RecordingMode>();
  const [meetings, setMeetings] = useState<Meeting[]>([]),
    [current, setCurrent] = useState<Meeting>(),
    [search, setSearch] = useState(""),
    [tab, setTab] = useState<"transcript" | "minutes" | "actions" | "map">(
      "transcript",
    );
  const [transcriptSearch, setTranscriptSearch] = useState("");
  const [speakerFilter, setSpeakerFilter] = useState("all");
  const [markerText, setMarkerText] = useState("");
  const [markerSaving, setMarkerSaving] = useState(false);
  const [markerRetry, setMarkerRetry] = useState(false);
  const [chunkTranscribing, setChunkTranscribing] = useState(false);
  const markerRequest = useRef<
    { id: string; meetingId: string; atMs: number; text: string } | undefined
  >(undefined);
  const markerLock = useRef(false);
  const markerPromise = useRef<Promise<unknown> | undefined>(undefined);
  const chunkTranscriptionLock = useRef(false);
  const [busy, setBusy] = useState(""),
    [loading, setLoading] = useState(native),
    [loadFailed, setLoadFailed] = useState(false),
    [error, setError] = useState(""),
    [dirty, setDirty] = useState(false),
    [recording, setRecording] = useState(false),
    [elapsed, setElapsed] = useState(0),
    [pendingTranscripts, setPendingTranscripts] = useState<PendingTranscript[]>([]),
    [speakerError, setSpeakerError] = useState(""),
    [realtimeStatus, setRealtimeStatus] = useState(""),
    [chatId, setChatId] = useState(""),
    [deleting, setDeleting] = useState(false),
    [retranscribing, setRetranscribing] = useState(false),
    [unsavedAudio, setUnsavedAudio] = useState<
      { id: string; base64: string }[]
    >([]),
    [audioUrl, setAudioUrl] = useState("");
  const recorder = useRef<MeetingRecorder | undefined>(undefined),
    mounted = useRef(true),
    currentRef = useRef(current),
    started = useRef(0),
    baseDuration = useRef(0),
    audioRef = useRef("");
  const audioElement = useRef<HTMLAudioElement>(null);
  const audioRequest = useRef<Promise<string | undefined> | undefined>(
    undefined,
  );
  const audioGeneration = useRef(0);
  const pendingSeek = useRef<number | undefined>(undefined);
  const actionLock = useRef(false);
  const session = useRef<
      | {
          workspaceId: string;
          id: string;
          sessionId: string;
          failed: boolean;
          channel: Channel<MeetingRealtimeEvent>;
        }
      | undefined
    >(undefined),
    epoch = useRef(0),
    stopping = useRef<Promise<void> | undefined>(undefined);
  currentRef.current = current;
  useEffect(() => {
    // 会议页面离开后仍挂载，返回时重新读取本机模型状态与已保存的语音设置。
    if (!native || !workspaceId || pathname !== "/meetings") return;
    setSpeechStatus(undefined);
    setSpeakerStatus(undefined);
    setSpeechStatusFailed(false);
    setSpeechPreferences(undefined);
    setSpeechPreferencesError(false);
    let cancelled = false;
    let timer: number | undefined;
    let speakerTimer: number | undefined;
    let requestedWarmup = false;
    async function refreshSpeechStatus(model: LocalMeetingSpeechModelId, keepPolling = false) {
      try {
        const status = await api.meetingSpeechStatus(model);
        if (cancelled) return;
        setSpeechStatus(status);
        setSpeechStatusFailed(false);
        keepPolling = status.state === "downloading" || status.runtime === "loading";
        if (!status.unsupportedReason && status.state === "ready" && status.runtime === "cold" && !requestedWarmup) {
          requestedWarmup = true;
          await api.meetingSpeechPrepare(workspaceId);
          keepPolling = true;
        }
      } catch {
        if (cancelled) return;
        setSpeechStatus(undefined);
        setSpeechStatusFailed(true);
      }
      if (!cancelled && keepPolling)
        timer = window.setTimeout(() => void refreshSpeechStatus(model, true), 1000);
    }
    async function refreshSpeakerStatus() {
      try {
        const status = await api.meetingSpeakerModelStatus();
        if (cancelled) return;
        setSpeakerStatus(status);
        if (status.state === "downloading")
          speakerTimer = window.setTimeout(() => void refreshSpeakerStatus(), 1500);
      } catch {
        if (!cancelled) setSpeakerStatus(undefined);
      }
    }
    void api.meetingSpeechPreferences(workspaceId).then(
      (preferences) => {
        if (cancelled) return;
        setSpeechPreferences(preferences);
        if (preferences.mode === "local") {
          void refreshSpeechStatus(preferences.localModelId ?? defaultLocalMeetingSpeechModelId);
          if (preferences.localSpeakerDiarization) void refreshSpeakerStatus();
        }
      },
      () => {
        if (!cancelled) setSpeechPreferencesError(true);
      },
    );
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      window.clearTimeout(speakerTimer);
    };
  }, [pathname, workspaceId]);
  useEffect(() => {
    setTranscriptSearch("");
    setSpeakerFilter("all");
    setMarkerText("");
    setMarkerRetry(false);
    markerRequest.current = undefined;
  }, [current?.id]);
  const apiSpeechId = speechPreferences?.mode === "api" ? speechPreferences.modelId : "";
  const speechModel = apiSpeechId
    ? eligibleMeetingSpeechModels(data).find((model) => model.id === apiSpeechId)
    : undefined;
  const selectedSpeech = speechModel?.id ?? "";
  const localModelId = (speechPreferences?.mode === "local" ? speechPreferences.localModelId : undefined)
    ?? defaultLocalMeetingSpeechModelId;
  const localModelName = localMeetingSpeechModels[localModelId].name;
  const localDiarization = speechPreferences?.mode === "local" && !!speechPreferences.localSpeakerDiarization;
  const speakerModelReady = speakerStatus?.state === "ready";
  const configuredMode = speechPreferences?.mode === "api" ? "chunks" : "realtime";
  const configuredReady = speechPreferences?.mode === "api"
    ? !!speechModel
    : speechPreferences?.mode === "local" && speechStatus?.state === "ready" && !speechStatus.unsupportedReason;
  const mode = activeMode ?? (!onlyRecord && configuredReady ? configuredMode : "record");
  const recordingModes = [
    {
      value: configuredMode,
      label: configuredMode === "chunks" ? "API 分片转录" : "本机实时转录",
      disabled: !configuredReady,
    },
    { value: "record", label: "仅录音" },
  ];
  useEffect(() => setOnlyRecord(false), [configuredMode, apiSpeechId, localModelId]);
  const chats = data.models.filter(
      (m) =>
        m.enabled &&
        (m.capability === "chat" || m.capability === "vision") &&
        data.providers.some((p) => p.id === m.providerId && p.enabled),
    );
  const locked = !!busy || recording || loading || markerSaving;
  const selectedChat = chats.some((m) => m.id === chatId)
      ? chatId
      : chats[0]?.id || "";
  const canStartMode = (!!speechPreferences || speechPreferencesError) &&
    (mode === "record" || configuredReady);
  let localSpeechLabel = "正在检查模型…";
  if (speechStatusFailed) localSpeechLabel = "无法读取模型状态，可先仅录音";
  else if (speechStatus?.unsupportedReason) localSpeechLabel = `${speechStatus.unsupportedReason}，可先仅录音`;
  else if (speechStatus?.state === "ready" && speechStatus.runtime === "loading")
    localSpeechLabel = "正在后台预加载…";
  else if (speechStatus?.state === "ready" && speechStatus.runtime === "error")
    localSpeechLabel = "预加载失败，可在设置中重试";
  else if (speechStatus) localSpeechLabel = speechStatusLabels[speechStatus.state];
  const speechStatusLabel = speechPreferencesError
    ? "无法读取会议语音设置，可用系统默认麦克风仅录音"
    : !speechPreferences
      ? "正在读取会议语音设置…"
      : speechPreferences.mode === "api"
        ? speechModel
          ? `${speechModel.name} · API 分片转录`
          : "所选语音 API 已不可用，可先仅录音"
        : `${localModelName} · ${localSpeechLabel}`;
  function accept(m: Meeting) {
    if (!mounted.current) return;
    if (
      currentRef.current?.id === m.id &&
      currentRef.current.revision > m.revision
    )
      return;
    setMeetings((all) =>
      [m, ...all.filter((x) => x.id !== m.id)].sort(
        (a, b) => b.updatedAt - a.updatedAt,
      ),
    );
    if (!currentRef.current || currentRef.current.id === m.id) {
      currentRef.current = m;
      setCurrent(m);
    }
  }
  function fail(e: unknown) {
    if (mounted.current) {
      setError(String(e));
      notify(String(e));
    }
  }
  function clearAudio() {
    audioGeneration.current++;
    audioRequest.current = undefined;
    pendingSeek.current = undefined;
    audioElement.current?.pause();
    if (audioRef.current) URL.revokeObjectURL(audioRef.current);
    audioRef.current = "";
    setAudioUrl("");
  }
  async function loadMeetings() {
    const token = epoch.current;
    setLoading(true);
    setLoadFailed(false);
    setError("");
    try {
      const all = await api.meetingList(workspaceId);
      if (mounted.current && token === epoch.current) {
        setMeetings(all);
        setCurrent(all[0]);
      }
    } catch (e) {
      fail(e);
      if (mounted.current) setLoadFailed(true);
    } finally {
      if (mounted.current) setLoading(false);
    }
  }
  useEffect(() => {
    mounted.current = true;
    setRecording(false);
    setActiveMode(undefined);
    setBusy("");
    setPendingTranscripts([]);
    setSpeakerError("");
    setRealtimeStatus("");
    if (native) void loadMeetings();
    return () => {
      mounted.current = false;
      epoch.current++;
      const active = session.current;
      session.current = undefined;
      const rec = recorder.current;
      recorder.current = undefined;
      void (async () => {
        try {
          await rec?.stop();
        } catch (e) {
          notify(`录音收尾失败：${String(e)}`);
        }
        if (active)
          await api
            .meetingRealtimeStop(
              active.workspaceId,
              active.id,
              active.sessionId,
            )
            .catch((e) => notify(`实时转录收尾失败：${String(e)}`));
      })();
      audioGeneration.current++;
      audioElement.current?.pause();
      if (audioRef.current) URL.revokeObjectURL(audioRef.current);
      onRecordingChange?.(false);
    };
  }, [workspaceId]);
  useEffect(() => {
    onRecordingChange?.(recording || busy === "正在保存录音…");
  }, [recording, busy, onRecordingChange]);
  useEffect(() => {
    if (!recording) return;
    const timer = window.setInterval(
      () => setElapsed(Date.now() - started.current),
      500,
    );
    return () => clearInterval(timer);
  }, [recording]);
  async function run(label: string, action: () => Promise<void>) {
    setError("");
    setBusy(label);
    try {
      await action();
    } catch (e) {
      fail(e);
    } finally {
      if (mounted.current) setBusy("");
    }
  }
  async function save() {
    if (!current) return;
    await run("正在保存…", async () => {
      accept(await api.meetingSave(workspaceId, current));
      setDirty(false);
    });
  }
  async function stop() {
    if (stopping.current) return stopping.current;
    const rec = recorder.current,
      active = session.current,
      token = epoch.current;
    // 先登记停止任务，再允许 flush 同步触发失败回调，避免重复收尾。
    const finishing = Promise.resolve().then(async () => {
      setBusy("正在保存录音…");
      setRecording(false);
      let stopError: unknown;
      try {
        await rec?.stop();
      } catch (e) {
        stopError = e;
      }
      if (active) {
        let saved: Meeting | undefined;
        let finalized = false;
        try {
          saved = await api.meetingRealtimeStop(
            active.workspaceId,
            active.id,
            active.sessionId,
          );
          finalized = true;
        } catch (e) {
          stopError = e;
          try {
            saved = (await api.meetingList(active.workspaceId)).find(
              (m) => m.id === active.id,
            );
          } catch {
            /* 保留未确认的转录，避免读取失败掩盖收尾错误。 */
          }
        }
        if (saved && mounted.current && token === epoch.current) {
          const savedIds = new Set(saved.segments.map((segment) => segment.id));
          accept(saved);
          setPendingTranscripts((all) => all.flatMap((transcript) => {
            if (transcript.meetingId !== active.id) return [transcript];
            if (finalized && transcript.sessionId === active.sessionId) return [];
            const segments = transcript.segments.filter((segment) => !savedIds.has(segment.id));
            return segments.length ? [{ ...transcript, segments }] : [];
          }));
        }
      }
      if (mounted.current && token === epoch.current) {
        if (stopError) {
          fail(`录音收尾未全部完成：${String(stopError)}`);
        } else notify("录音已保存");
        setRealtimeStatus("");
        setSpeakerError("");
        setBusy("");
        setActiveMode(undefined);
        setElapsed(0);
      }
      if (session.current === active) session.current = undefined;
      if (recorder.current === rec) recorder.current = undefined;
    });
    stopping.current = finishing;
    try {
      await finishing;
    } finally {
      if (stopping.current === finishing) stopping.current = undefined;
    }
  }
  async function start() {
    if (
      !current ||
      dirty ||
      markerLock.current ||
      !native ||
      busy ||
      recording ||
      !canStartMode
    )
      return;
    const id = current.id,
      model = selectedSpeech,
      method = mode,
      inputDeviceId = speechPreferences?.inputDeviceId,
      token = epoch.current;
    const duration = current.durationMs;
    setActiveMode(method);
    setBusy(
      method === "realtime"
        ? `正在确认 ${localModelName} 状态…`
        : "正在打开麦克风…",
    );
    setError("");
    setSpeakerError("");
    setRealtimeStatus("");
    let active: typeof session.current;
    let transcriptionFailed = false;
    let speechReady = method !== "realtime";
    const valid = () => mounted.current && token === epoch.current;
    try {
      if (method === "realtime") {
        const status = await api.meetingSpeechStatus(localModelId);
        if (!valid()) return;
        setSpeechStatus(status);
        setSpeechStatusFailed(false);
        if (status.unsupportedReason) throw new Error(status.unsupportedReason);
        if (status.state !== "ready") throw new Error(`${localModelName} 尚未安装完成。`);
        if (status.runtime !== "ready") setBusy(`正在等候 ${localModelName} 就绪…`);
        const channel = new Channel<MeetingRealtimeEvent>();
        active = {
          workspaceId,
          id,
          sessionId: crypto.randomUUID(),
          failed: false,
          channel,
        };
        const linked = active;
        session.current = linked;
        channel.onmessage = (event) => {
          if (
            !valid() ||
            session.current !== linked ||
            event.sessionId !== linked.sessionId ||
            event.workspaceId !== workspaceId ||
            event.meetingId !== id
          )
            return;
          if (event.type === "delta") {
            const text = event.text?.trim();
            // 原生事件为当前会话的完整片段快照，允许原地修改和合并已有片段。
            const segments = event.lines?.length ? event.lines : text ? [{
              id: `realtime:${linked.sessionId}:0`,
              startMs: duration,
              endMs: Math.max(duration, currentRef.current?.durationMs ?? duration),
              text,
            }] : [];
            setPendingTranscripts((all) => [
              ...all.filter((transcript) => transcript.sessionId !== linked.sessionId),
              ...(segments.length ? [{ meetingId: id, sessionId: linked.sessionId, segments }] : []),
            ]);
            setRealtimeStatus(
              linked.failed
                ? "实时转录已中断，片段将在收尾时保存"
                : "正在实时识别",
            );
          } else if (event.type === "speakerError") {
            setSpeakerError(event.message || "说话人识别暂不可用");
          } else {
            linked.failed = true;
            setRealtimeStatus("实时转录已中断，录音继续保存");
            fail(event.message || "实时转录失败");
          }
        };
        await api.meetingRealtimeStart(
          workspaceId,
          id,
          "local",
          linked.sessionId,
          channel,
        );
        if (!valid()) {
          await api.meetingRealtimeStop(workspaceId, id, linked.sessionId);
          return;
        }
        if (linked.failed)
          throw new Error("本机实时转录启动失败");
        speechReady = true;
        setBusy("正在打开麦克风…");
        setRealtimeStatus("等待语音…");
      }
      const rec = new MeetingRecorder(
        async (chunkId, base64) => {
          let saved: Meeting;
          try {
            saved = await api.meetingAppendAudio(
              workspaceId,
              id,
              chunkId,
              base64,
            );
          } catch (e) {
            if (valid())
              setUnsavedAudio((all) => [...all, { id: chunkId, base64 }]);
            throw e;
          }
          if (valid()) {
            clearAudio();
            accept(saved);
          }
          if (method === "realtime" && active && !active.failed) {
            try {
              await api.meetingRealtimePush(
                workspaceId,
                id,
                active.sessionId,
                chunkId,
              );
            } catch (e) {
              active.failed = true;
              if (valid()) {
                setRealtimeStatus(
                  "实时转录已中断，录音继续保存",
                );
                fail(`音频已保存，但实时转录失败：${String(e)}`);
              }
            }
          } else if (method === "chunks" && !transcriptionFailed) {
            // 分片识别使用会议版本锁；已有打点先落库，识别期间不再写入标记。
            chunkTranscriptionLock.current = true;
            if (valid()) setChunkTranscribing(true);
            try {
              await markerPromise.current;
              const result = await api.meetingTranscribe(
                workspaceId,
                id,
                model,
                chunkId,
              );
              if (valid()) accept(result);
            } catch (e) {
              transcriptionFailed = true;
              if (valid())
                fail(`音频已保存，但转录失败；可稍后重新转录：${String(e)}`);
            } finally {
              chunkTranscriptionLock.current = false;
              if (valid()) setChunkTranscribing(false);
            }
          }
        },
        (e) => {
          if (valid()) {
            fail(e);
            void stop();
          }
        },
        method === "realtime" ? 0.16 : 10,
        inputDeviceId,
      );
      recorder.current = rec;
      await rec.start();
      if (!valid()) {
        await rec.stop();
        return;
      }
      started.current = Date.now();
      baseDuration.current = duration;
      setElapsed(0);
      setRecording(true);
      if (method === "realtime") setTab("transcript");
    } catch (e) {
      if (valid()) {
        setRealtimeStatus("");
        setActiveMode(undefined);
        fail(
          speechReady
            ? `麦克风启动失败，请检查设备与录音权限。${String(e)}`
            : `本机实时转录启动失败，可在会议语音设置中查看状态，或切换为仅录音。${String(e)}`,
        );
      }
      const failedSession = active;
      if (failedSession)
        await api
          .meetingRealtimeStop(workspaceId, id, failedSession.sessionId)
          .then((saved) => {
            if (!valid()) return;
            accept(saved);
            setPendingTranscripts((all) => all.filter((transcript) => transcript.sessionId !== failedSession.sessionId));
          })
          .catch(() => undefined);
      if (session.current === active) session.current = undefined;
      recorder.current = undefined;
    } finally {
      if (valid()) setBusy("");
    }
  }
  async function retranscribe() {
    if (!current || !selectedSpeech) return;
    await run("正在完整转录…", async () => {
      accept(
        await api.meetingTranscribe(workspaceId, current.id, selectedSpeech),
      );
      setRetranscribing(false);
    });
  }
  async function recognizeSpeakers() {
    if (!current || !speakerModelReady || dirty || locked) return;
    await run("正在识别说话人…", async () => {
      const result = await api.meetingDiarize(workspaceId, current.id, current.revision);
      accept(result.meeting);
      notify(result.assigned ? `已标注 ${result.assigned} 段说话人` : "暂无可新增的说话人标注");
    });
  }
  function edit(value: Partial<Meeting>) {
    setCurrent(
      (m) =>
        m && {
          ...m,
          ...value,
          analysis:
            m.analysis &&
            (value.segments ||
              value.speakerNames ||
              value.context ||
              value.title)
              ? { ...m.analysis, stale: true }
              : m.analysis,
        },
    );
    setDirty(true);
  }
  const context = { ...defaultMeetingContext(), ...current?.context };
  function editContext(value: MeetingContext) {
    edit({ context: value });
  }
  async function loadAudio(): Promise<string | undefined> {
    if (audioRef.current) return audioRef.current;
    if (audioRequest.current) return audioRequest.current;
    if (!current) return;
    const meetingId = current.id;
    const token = epoch.current;
    const generation = audioGeneration.current;
    const request = (async () => {
      const audio = await api.meetingAudio(workspaceId, meetingId);
      if (
        !mounted.current ||
        token !== epoch.current ||
        generation !== audioGeneration.current ||
        currentRef.current?.id !== meetingId
      )
        return;
      const bytes = Uint8Array.from(atob(audio.audioBase64), (c) =>
        c.charCodeAt(0),
      );
      const url = URL.createObjectURL(
        new Blob([bytes], { type: audio.mimeType }),
      );
      audioRef.current = url;
      setAudioUrl(url);
      return url;
    })();
    audioRequest.current = request;
    try {
      return await request;
    } finally {
      if (audioRequest.current === request) audioRequest.current = undefined;
    }
  }
  function seekAudio() {
    const player = audioElement.current;
    const seconds = pendingSeek.current;
    if (
      !player ||
      seconds === undefined ||
      player.readyState < 1 ||
      player.src !== audioRef.current
    )
      return;
    pendingSeek.current = undefined;
    player.currentTime = Math.min(
      seconds,
      Number.isFinite(player.duration) ? player.duration : seconds,
    );
    void player.play().catch(() => {
      if (mounted.current) notify("录音已定位，请点击播放器继续播放。");
    });
  }
  async function playSegment(startMs: number) {
    if (locked || !current?.audioChunks) return;
    pendingSeek.current = startMs / 1000;
    await run("正在定位录音…", async () => {
      try {
        if (await loadAudio()) seekAudio();
      } catch (error) {
        pendingSeek.current = undefined;
        throw error;
      }
    });
  }
  async function addMarker() {
    const meeting = currentRef.current;
    if (
      !meeting ||
      !meeting.durationMs ||
      markerLock.current ||
      chunkTranscriptionLock.current ||
      loading ||
      dirty ||
      busy
    )
      return;
    const token = epoch.current;
    const valid = () =>
      mounted.current &&
      token === epoch.current &&
      currentRef.current?.id === meeting.id;
    const request = markerRequest.current ?? {
      id: crypto.randomUUID(),
      meetingId: meeting.id,
      atMs: meeting.durationMs,
      text: markerText.trim() || "重点",
    };
    markerRequest.current = request;
    markerLock.current = true;
    setError("");
    setMarkerSaving(true);
    try {
      const operation = api.meetingAddMarker(
        workspaceId,
        request.meetingId,
        request.id,
        request.atMs,
        request.text,
      );
      // 录音分片等待写入结束即可，标记失败由本入口展示且沿稳定 ID 手动重试。
      markerPromise.current = operation.then(
        () => undefined,
        () => undefined,
      );
      const saved = await operation;
      if (valid()) {
        accept(saved);
        markerRequest.current = undefined;
        setMarkerRetry(false);
        setMarkerText("");
      }
    } catch (error) {
      if (valid()) {
        setMarkerRetry(true);
        fail(
          `重点标记保存未确认，请点击“重试标记”；录音与已保存文字仍保留。${String(error)}`,
        );
      }
    } finally {
      markerLock.current = false;
      markerPromise.current = undefined;
      if (valid()) setMarkerSaving(false);
    }
  }
  async function changeAction(action: MeetingAction, completed?: boolean) {
    if (
      !current ||
      !action.id ||
      actionLock.current ||
      locked ||
      dirty ||
      current.analysis?.stale
    )
      return;
    const meeting = current;
    const token = epoch.current;
    const valid = () =>
      mounted.current &&
      token === epoch.current &&
      currentRef.current?.id === meeting.id;
    actionLock.current = true;
    try {
      await run(
        completed === undefined ? "正在转为任务…" : "正在更新行动项…",
        async () => {
          if (completed !== undefined) {
            const saved = await api.meetingActionComplete(
              workspaceId,
              meeting.id,
              action.id!,
              completed,
              meeting.revision,
            );
            if (valid()) accept(saved);
            return;
          }
          const result = await api.meetingActionTask(
            workspaceId,
            meeting.id,
            action.id!,
            meeting.revision,
          );
          if (!valid()) return;
          accept(result.meeting);
          try {
            const refreshed = await onTasksChanged?.();
            if (
              refreshed &&
              typeof refreshed === "object" &&
              "error" in refreshed &&
              refreshed.error
            )
              throw refreshed.error;
          } catch {
            if (valid())
              fail(
                "任务已创建，但任务列表刷新失败。请刷新后到“我的任务”查看，不要重复创建。",
              );
            return;
          }
          if (valid())
            notify("行动项已转为本地任务，负责人和期限已保留在任务说明中。");
        },
      );
    } finally {
      actionLock.current = false;
    }
  }
  const shown = meetings.filter((m) =>
    `${m.title} ${m.segments.map((s) => s.text).join(" ")}`
      .toLowerCase()
      .includes(search.toLowerCase()),
  );
  const savedSegments = current?.segments ?? [];
  const savedSegmentIds = new Set(savedSegments.map((segment) => segment.id));
  const pendingSegments = pendingTranscripts
    .filter((transcript) => transcript.meetingId === current?.id)
    .flatMap((transcript) => transcript.segments)
    .filter((segment) => !savedSegmentIds.has(segment.id));
  const transcriptSegments = [...savedSegments, ...pendingSegments];
  const speakerCounts = new Map<string, number>();
  for (const segment of transcriptSegments) {
    const key = segment.speaker ? `speaker:${segment.speaker}` : "unmarked";
    speakerCounts.set(key, (speakerCounts.get(key) ?? 0) + 1);
  }
  const speakerFilterAvailable =
    speakerFilter === "all" || speakerCounts.has(speakerFilter);
  useEffect(() => {
    if (!speakerFilterAvailable) setSpeakerFilter("all");
  }, [speakerFilterAvailable]);
  const segmentSpeakerLabel = (key: string) =>
    key === "unmarked"
      ? "未标记说话人"
      : current?.speakerNames[key.slice(8)] ||
        speakerLabel(key.slice(8), transcriptSegments);
  const transcriptQuery = transcriptSearch.trim().toLocaleLowerCase();
  const newestSegments = transcriptSegments
    .filter(
      (segment) =>
        (speakerFilter === "all" ||
          (segment.speaker ? `speaker:${segment.speaker}` : "unmarked") ===
            speakerFilter) &&
        segment.text
          .toLocaleLowerCase()
          .includes(transcriptQuery),
    )
    .sort((a, b) => b.startMs - a.startMs);
  const pendingActions =
    current?.analysis?.actionItems.filter((action) =>
      action.taskId
        ? data.tasks.find((task) => task.id === action.taskId)?.status !==
          "done"
        : !action.completed,
    ).length ?? 0;
  const recordingStatus = {
    realtime: `${localModelName} · ${realtimeStatus || "本机实时转录中"}`,
    chunks: "录音中 · 每约 10 秒通过 API 转录",
    record: "录音中 · 仅保存音频",
  }[mode];
  const speechSettingsLink = !locked && native && (
    <Link className="meeting-speech-link" to="/settings?tab=meeting-speech">
      <Settings2 size={14} /> 会议语音设置
    </Link>
  );
  return (
    <section className="meetings-page">
      <aside className="meeting-sidebar">
        <header>
          <h2>会议</h2>
          <button
            className="primary"
            disabled={!native || locked || dirty}
            onClick={() =>
              run("正在新建…", async () => {
                const m = await api.meetingSave(workspaceId, makeMeeting());
                accept(m);
                setCurrent(m);
                setTab("transcript");
              })
            }
          >
            <Plus size={15} />
            新建
          </button>
        </header>
        <label className="meeting-search">
          <Search size={15} />
          <input
            aria-label="搜索会议"
            placeholder="搜索会议或转录内容"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </label>
        <div className="meeting-list">
          {loading && (
            <p className="meeting-empty-small" role="status">
              正在加载会议…
            </p>
          )}
          {loadFailed && (
            <button onClick={() => void loadMeetings()}>重新加载会议</button>
          )}
          {shown.map((m) => (
            <button
              disabled={locked || dirty}
              className={`meeting-list-item ${current?.id === m.id ? "selected" : ""}`}
              key={m.id}
              onClick={() => {
                setCurrent(m);
                setTab("transcript");
                setError("");
                clearAudio();
              }}
            >
              <strong>{m.title}</strong>
              <span>
                {new Date(m.createdAt).toLocaleDateString("zh-CN")} ·{" "}
                {meetingTime(m.durationMs)}
              </span>
              <p>
                {m.analysis?.summary || m.segments[0]?.text || "还没有转录内容"}
              </p>
            </button>
          ))}
          {!shown.length && !loading && !loadFailed && (
            <p className="meeting-empty-small">
              {search ? "没有匹配的会议" : "新建会议，记录每一个重要决定。"}
            </p>
          )}
        </div>
      </aside>
      <div className="meeting-detail">
        {!native && (
          <p className="meeting-notice">
            请在栖点桌面应用中使用会议录音与本地保存。
          </p>
        )}
        {error && (
          <div className="meeting-error" role="alert">
            {error}
          </div>
        )}
        {unsavedAudio.length > 0 && (
          <div className="meeting-error" role="alert">
            有 {unsavedAudio.length}{" "}
            个录音片段保存结果未确认，请先导出备份再离开工作空间。
            {unsavedAudio.map((chunk, i) => (
              <button
                key={chunk.id}
                onClick={() =>
                  download(
                    Uint8Array.from(atob(chunk.base64), (c) => c.charCodeAt(0)),
                    "audio/wav",
                    `未确认录音-${i + 1}.wav`,
                  )
                }
              >
                导出片段 {i + 1}
              </button>
            ))}
          </div>
        )}
        {!current ? (
          <div className="meeting-empty">
            <AudioLines size={38} />
            <h2>让讨论留下清晰的结论</h2>
            <p>录制会议、整理发言，再生成纪要与会议脑图。</p>
            <p>从左侧新建一场会议开始。</p>
            {native && <p role="status">{speechStatusLabel}</p>}
            {speechSettingsLink}
          </div>
        ) : (
          <>
            <header className="meeting-heading">
              <input
                aria-label="会议名称"
                value={current.title}
                disabled={locked}
                onChange={(e) => edit({ title: e.target.value })}
              />
              <div>
                <button disabled={!dirty || locked} onClick={save}>
                  <Save size={15} />
                  保存{dirty ? "修改" : ""}
                </button>
                <button
                  className="icon-button"
                  title="删除会议"
                  disabled={locked || dirty}
                  onClick={() => setDeleting(true)}
                >
                  <Trash2 className="danger" size={16} />
                </button>
              </div>
            </header>
            <MeetingContextFields
              key={current.id}
              meeting={current}
              value={context}
              disabled={locked}
              onChange={editContext}
            />
            <div className="meeting-record-bar">
              <button
                className={recording ? "meeting-stop" : "primary"}
                disabled={
                  !native || !!busy ||
                  dirty ||
                  (!recording && (markerSaving || !canStartMode))
                }
                onClick={recording ? stop : start}
              >
                {recording ? <Square size={15} /> : <Mic size={16} />}{" "}
                {recording
                  ? "结束录音"
                  : current.audioChunks
                    ? "继续录音"
                    : "开始录音"}
              </button>
              <span className={`meeting-duration ${recording ? "active" : ""}`}>
                {recording && <i />}
                {meetingTime(
                  recording
                    ? baseDuration.current + elapsed
                    : current.durationMs,
                )}
              </span>
              <div className="meeting-record-options">
                <label>
                  录音模式
                  <GlassSelect
                    aria-label="录音模式"
                    value={mode}
                    disabled={locked}
                    options={recordingModes}
                    onValueChange={(value) => setOnlyRecord(value === "record")}
                  />
                </label>
                {current.audioChunks > 0 && (
                  <button
                    disabled={locked}
                    onClick={() =>
                      run("正在读取录音…", async () => {
                        await loadAudio();
                      })
                    }
                  >
                    <Play size={15} /> 播放 / 下载录音
                  </button>
                )}
              </div>
            </div>
            <div className="meeting-record-status">
              <span className="meeting-state" role="status">
                {busy || (recording ? recordingStatus : speechStatusLabel)}
              </span>
              {speechSettingsLink}
              {localDiarization && !speakerModelReady && !locked && (
                <span className="meeting-hint">说话人模型未就绪，仍可实时转录</span>
              )}
              {activeMode === "realtime" && speakerError && (
                <span className="meeting-hint" role="status">{speakerError}。实时文字转录继续。</span>
              )}
              {!recording && !!current.audioChunks && (
                <span className="meeting-record-count">已保存 {current.audioChunks} 个音频片段</span>
              )}
            </div>
            <div className="meeting-marker-bar">
              <label className="meeting-marker-input">
                <BookmarkPlus size={15} />
                <input
                  aria-label="重点备注"
                  value={markerText}
                  maxLength={500}
                  placeholder="补充一句重点，或直接打点"
                  disabled={
                    loading || dirty || !!busy || markerSaving || markerRetry
                  }
                  onChange={(event) => setMarkerText(event.target.value)}
                  onKeyDown={(event) => {
                    if (
                      event.key === "Enter" &&
                      !event.nativeEvent.isComposing
                    ) {
                      event.preventDefault();
                      void addMarker();
                    }
                  }}
                />
              </label>
              <button
                disabled={
                  loading ||
                  dirty ||
                  !!busy ||
                  markerSaving ||
                  chunkTranscribing ||
                  !current.durationMs
                }
                title={
                  chunkTranscribing
                    ? "当前分片正在识别，完成后可继续标记"
                    : "记录最近已保存音频的位置，方便会后回听"
                }
                onClick={() => void addMarker()}
              >
                {markerSaving
                  ? "正在保存标记…"
                  : markerRetry
                    ? "重试标记"
                    : "标记重点"}
              </button>
            </div>
            {audioUrl && (
              <div className="meeting-audio">
                <audio
                  controls
                  ref={audioElement}
                  src={audioUrl}
                  onLoadedMetadata={seekAudio}
                  aria-label="会议录音播放器"
                />
                <a href={audioUrl} download={`${current.title}.wav`}>
                  下载 WAV
                </a>
              </div>
            )}
            <nav className="meeting-tabs" aria-label="会议内容">
              {(
                [
                  ["transcript", "转录", AudioLines],
                  ["minutes", "会议纪要", FileText],
                  ["actions", "行动项", ListChecks],
                  ["map", "会议脑图", GitBranch],
                ] as const
              ).map(([key, label, Icon]) => (
                <button
                  key={key}
                  aria-current={tab === key ? "page" : undefined}
                  onClick={() => setTab(key)}
                >
                  <Icon size={16} />
                  {label}
                  {key === "transcript" && (
                    <span>{transcriptSegments.length}</span>
                  )}
                  {key === "actions" && <span>{pendingActions}</span>}
                </button>
              ))}
            </nav>
            <div className="meeting-content">
              {tab !== "transcript" && (
                <div className="meeting-analysis-bar">
                  <label>
                    纪要模型
                    <GlassSelect
                      aria-label="纪要模型"
                      options={chats.map((m) => ({ value: m.id, label: m.name }))}
                      value={selectedChat}
                      onValueChange={setChatId}
                      disabled={locked}
                      placeholder="选择文本模型"
                    />
                  </label>
                  <label>
                    整理方式
                    <GlassSelect
                      aria-label="纪要整理方式"
                      value={context.summaryStyle}
                      disabled={locked}
                      options={[
                        { value: "brief", label: "简明" },
                        { value: "standard", label: "标准" },
                        { value: "detailed", label: "详细" },
                      ]}
                      onValueChange={(value) =>
                        editContext({
                          ...context,
                          summaryStyle: value as MeetingContext["summaryStyle"],
                        })
                      }
                    />
                  </label>
                  <button
                    disabled={
                      locked || dirty || !selectedChat || !current.segments.length
                    }
                    onClick={() =>
                      run("正在生成会议纪要与脑图…", async () => {
                        accept(
                          await api.meetingGenerate(
                            workspaceId,
                            current.id,
                            selectedChat,
                            current.revision,
                          ),
                        );
                        setTab("minutes");
                      })
                    }
                  >
                    <WandSparkles size={15} />
                    生成纪要与脑图
                  </button>
                  <button
                    disabled={locked}
                    onClick={() =>
                      download(
                        meetingMarkdown(current),
                        "text/markdown;charset=utf-8",
                        `${current.title}.md`,
                      )
                    }
                  >
                    <Download size={15} />
                    导出纪要
                  </button>
                </div>
              )}
              {tab !== "transcript" && current.analysis?.stale && (
                <p className="meeting-notice" role="status">
                  会议资料、转录或录音已更新，请重新生成。当前显示与导出的是旧版本内容。
                </p>
              )}
              {tab === "transcript" ? (
                <>
                  {!activeMode && current.audioChunks > 0 && speechModel && (
                    <section className="meeting-retranscribe-bar" aria-label="会后转录">
                      <button
                        disabled={locked || dirty}
                        onClick={() =>
                          current.segments.length
                            ? setRetranscribing(true)
                            : void retranscribe()
                        }
                      >
                        <AudioLines size={15} />
                        {speechModel.speechMode === "diarization"
                          ? "重新转录并识别人声"
                          : "重新转录"}
                      </button>
                      {current.segments.length > 0 && (
                        <span className="meeting-hint">将替换校对文本并重置说话人姓名。</span>
                      )}
                    </section>
                  )}
                  {!activeMode && current.audioChunks > 0 && current.segments.length > 0 && localDiarization && (
                    <section className="meeting-retranscribe-bar" aria-label="本机说话人识别">
                      <button disabled={locked || dirty || !speakerModelReady} onClick={() => void recognizeSpeakers()}>
                        <AudioLines size={15} />识别说话人
                      </button>
                      <span className="meeting-hint">为可以确认的转录片段补标说话人。</span>
                    </section>
                  )}
                  {!!current.markers?.length && (
                    <section className="meeting-markers" aria-label="会议重点">
                      <h3>
                        重点记录 <span>{current.markers.length}</span>
                      </h3>
                      <ul>
                        {[...current.markers]
                          .sort((a, b) => b.atMs - a.atMs)
                          .map((marker) => (
                            <li key={marker.id}>
                              <button
                                className="meeting-segment-play"
                                disabled={locked || !current.audioChunks}
                                aria-label={`回听重点 ${meetingTime(marker.atMs)}：${marker.text}`}
                                onClick={() => void playSegment(marker.atMs)}
                              >
                                <Play size={12} />
                                <time>{meetingTime(marker.atMs)}</time>
                              </button>
                              <span>{marker.text}</span>
                            </li>
                          ))}
                      </ul>
                    </section>
                  )}
                  {transcriptSegments.length > 0 && (
                    <>
                      <div className="meeting-transcript-tools">
                        <label className="meeting-search">
                          <Search size={14} />
                          <input
                            aria-label="搜索本场转录"
                            placeholder="搜索本场转录"
                            value={transcriptSearch}
                            onChange={(event) =>
                              setTranscriptSearch(event.target.value)
                            }
                          />
                        </label>
                        <GlassSelect
                          aria-label="筛选说话人"
                          value={speakerFilter}
                          onValueChange={setSpeakerFilter}
                          options={[
                            {
                              value: "all",
                              label: `全部说话人 · ${transcriptSegments.length} 段`,
                            },
                            ...[...speakerCounts].map(([value, count]) => ({
                              value,
                              label: `${segmentSpeakerLabel(value)} · ${count} 段`,
                            })),
                          ]}
                        />
                        <span className="meeting-hint">
                          {newestSegments.length} / {transcriptSegments.length} 段
                        </span>
                      </div>
                      {activeMode !== "realtime" && (
                        <p className="meeting-hint">
                          点击时间可从已保存的片段回听，修改文本或说话人名称后请保存。
                        </p>
                      )}
                      {!activeMode && pendingSegments.length > 0 && (
                        <p className="meeting-notice" role="status">
                          {pendingSegments.length} 段转录尚未确认保存，可选中文字复制留存。
                        </p>
                      )}
                      {!newestSegments.length && (
                        <p className="meeting-empty-small">
                          没有符合条件的转录。请调整搜索或说话人筛选。
                        </p>
                      )}
                    </>
                  )}
                  {!transcriptSegments.length && activeMode !== "realtime" && (
                      <div className="meeting-empty">
                        <AudioLines size={30} />
                        <h3>
                          {recording ? "正在录音…" : "转录内容将在这里逐段出现"}
                        </h3>
                        <p>
                          {recording
                            ? mode === "chunks"
                              ? "每约 10 秒显示一段转录。"
                              : "正在保存音频。"
                          : current.audioChunks
                            ? speechModel
                              ? "可在上方重新转录已保存的录音。"
                              : "可在会议语音设置中启用 API 后重新转录。"
                              : mode === "record"
                                ? "仅录音会保存音频，供后续回听或转录。"
                                : "开始录音后，文字会在这里显示。"}
                        </p>
                      </div>
                    )}
                  {newestSegments.map((s, index) => {
                    const pending = !savedSegmentIds.has(s.id);
                    return (
                      <div className="meeting-segment" key={s.id}>
                        <div>
                          <button
                            className="meeting-segment-play"
                            aria-label={`从 ${meetingTime(s.startMs)} 回听`}
                            title="从这个转录片段开始回听"
                            disabled={locked || pending || !current.audioChunks}
                            onClick={() => void playSegment(s.startMs)}
                          >
                            <Play size={12} />
                            <time>{meetingTime(s.startMs)}</time>
                          </button>
                          {s.speaker ? (
                            <input
                              aria-label={`说话人 ${s.speaker} 的名称`}
                              disabled={locked || pending}
                              value={segmentSpeakerLabel(`speaker:${s.speaker}`)}
                              onChange={(e) =>
                                edit({
                                  speakerNames: {
                                    ...current.speakerNames,
                                    [s.speaker!]: e.target.value,
                                  },
                                })
                              }
                            />
                          ) : (
                            <span>未标记说话人</span>
                          )}
                        </div>
                        <TranscriptInput
                          aria-label={`第 ${index + 1} 段转录`}
                          rows={1}
                          readOnly={pending}
                          disabled={locked && !pending}
                          value={s.text}
                          onChange={pending ? undefined : (e) =>
                            edit({
                              segments: current.segments.map((x) =>
                                x.id === s.id
                                  ? { ...x, text: e.target.value }
                                  : x,
                              ),
                            })
                          }
                        />
                      </div>
                    );
                  })}
                </>
              ) : tab === "minutes" ? (
                current.analysis ? (
                  <article className="meeting-minutes">
                    {(current.analysis.background || context.background) && (
                      <>
                        <h3>会议背景</h3>
                        <p>
                          {current.analysis.background || context.background}
                        </p>
                      </>
                    )}
                    {(current.analysis.participants?.length ||
                      context.participants.length) > 0 && (
                      <>
                        <h3>参会人员</h3>
                        <p>
                          {(current.analysis.participants?.length
                            ? current.analysis.participants
                            : context.participants
                          ).join("、")}
                        </p>
                      </>
                    )}
                    <h2>会议摘要</h2>
                    <p>{current.analysis.summary}</p>
                    <h3>会议结论</h3>
                    {current.analysis.decisions.length ? (
                      <ul>
                        {current.analysis.decisions.map((x, i) => (
                          <li key={i}>{x}</li>
                        ))}
                      </ul>
                    ) : (
                      <p className="meeting-hint">会议中未提取到明确结论。</p>
                    )}
                    <h3>行动项</h3>
                    {current.analysis.actionItems.length ? (
                      <ul>
                        {current.analysis.actionItems.map((x, i) => (
                          <li key={i}>
                            <strong>{x.task}</strong>
                            <span>
                              {[
                                x.owner && `负责人：${x.owner}`,
                                x.deadline && `期限：${x.deadline}`,
                              ]
                                .filter(Boolean)
                                .join(" · ") || "负责人和期限待确认"}
                            </span>
                          </li>
                        ))}
                      </ul>
                    ) : (
                      <p className="meeting-hint">会议中未提取到明确行动项。</p>
                    )}
                    {current.analysis.actionItems.length > 0 && (
                      <button
                        className="meeting-actions-link"
                        onClick={() => setTab("actions")}
                      >
                        <ListChecks size={15} />
                        管理行动项
                      </button>
                    )}
                    <footer>
                      AI 生成于{" "}
                      {new Date(current.analysis.generatedAt).toLocaleString(
                        "zh-CN",
                      )}
                      ，请结合转录核对。
                    </footer>
                  </article>
                ) : (
                  <div className="meeting-empty">
                    <FileText size={30} />
                    <h3>把讨论整理成下一步</h3>
                    <p>转录完成后，点击「生成纪要与脑图」。</p>
                  </div>
                )
              ) : tab === "actions" ? (
                <MeetingActions
                  meeting={current}
                  tasks={data.tasks}
                  disabled={locked || dirty}
                  canOpenTask={!!onOpenTask}
                  onComplete={(action, completed) =>
                    void changeAction(action, completed)
                  }
                  onCreateTask={(action) => void changeAction(action)}
                  onOpenTask={(id) => onOpenTask?.(id)}
                />
              ) : current.analysis ? (
                <>
                  <div className="meeting-map-tools">
                    <span>会议主题与关键分支</span>
                    <button
                      onClick={() => {
                        if (current.analysis?.stale)
                          notify(
                            "正在导出旧版本脑图，转录或录音已更新，请重新生成。",
                          );
                        download(
                          mindMapSvg(current.analysis!.mindMap),
                          "image/svg+xml",
                          `${current.title}-脑图${current.analysis?.stale ? "-旧版本" : ""}.svg`,
                        );
                      }}
                    >
                      <Download size={15} />
                      导出 SVG
                    </button>
                  </div>
                  <MindMap root={current.analysis.mindMap} />
                </>
              ) : (
                <div className="meeting-empty">
                  <GitBranch size={30} />
                  <h3>会议脑图尚未生成</h3>
                  <p>生成纪要时会同时整理主题关系。</p>
                </div>
              )}
            </div>
          </>
        )}
        {retranscribing && current && (
          <Modal
            title="重新转录会议"
            busy={!!busy}
            onClose={() => setRetranscribing(false)}
          >
            <p>
              重新转录将替换现有转录内容，包括手工校对文本，并重置说话人姓名。录音保持保存，现有纪要会标记为旧版本。
            </p>
            <div className="meeting-dialog-actions">
              <button
                disabled={!!busy}
                onClick={() => setRetranscribing(false)}
              >
                取消
              </button>
              <button
                className="primary"
                disabled={!!busy || !selectedSpeech}
                onClick={() => void retranscribe()}
              >
                继续重新转录
              </button>
            </div>
          </Modal>
        )}
        {deleting && current && (
          <Modal
            title="删除会议"
            onClose={() => setDeleting(false)}
            busy={!!busy}
          >
            <p>
              确认删除「{current.title}」及其录音、转录与纪要？此操作无法撤销。
            </p>
            <div className="meeting-dialog-actions">
              <button disabled={!!busy} onClick={() => setDeleting(false)}>
                取消
              </button>
              <button
                className="danger"
                disabled={!!busy}
                onClick={() =>
                  run("正在删除…", async () => {
                    await api.meetingDelete(
                      workspaceId,
                      current.id,
                      current.revision,
                    );
                    setMeetings((all) =>
                      all.filter((m) => m.id !== current.id),
                    );
                    setPendingTranscripts((all) => all.filter((transcript) => transcript.meetingId !== current.id));
                    setCurrent(undefined);
                    setDeleting(false);
                  })
                }
              >
                确认删除
              </button>
            </div>
          </Modal>
        )}
      </div>
    </section>
  );
}
