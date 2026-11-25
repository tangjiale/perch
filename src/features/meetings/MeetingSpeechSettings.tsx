import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { Link } from "react-router-dom";
import { AudioLines, CircleCheck, Cpu, Download, Plus, RefreshCw, Settings2, Trash2, Users } from "lucide-react";
import GlassSelect from "../../components/GlassSelect";
import Modal from "../../components/Modal";
import { api, native, type MeetingSpeechStatus } from "../../lib/api";
import {
  defaultLocalMeetingSpeechModelId,
  eligibleMeetingSpeechModels,
  localMeetingSpeechModelIds,
  localMeetingSpeechModels,
  type LocalMeetingSpeechModelId,
  type MeetingSpeechPreferences,
  type MeetingSpeechRecommendation,
} from "../../lib/meeting-speech";
import type { Provider, Snapshot } from "../../lib/types";
import MeetingMicrophoneSettings from "./MeetingMicrophoneSettings";
import "./meeting-speech-settings.css";

const statusLabel: Record<MeetingSpeechStatus["state"], string> = {
  missing: "未安装",
  downloading: "正在下载",
  ready: "已下载",
  error: "安装失败",
};

function modelStatusLabel(status?: MeetingSpeechStatus) {
  if (!status) return "正在检查模型…";
  if (status.state === "downloading" && status.totalBytes > 0 && status.downloadedBytes >= status.totalBytes) {
    return "正在校验并安装模型…";
  }
  return statusLabel[status.state];
}

function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KiB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GiB`;
}

interface Props {
  data: Snapshot;
  workspaceId: string;
  onConfigureProvider?: (provider?: Provider) => void;
}

const modeOptions = [
  { value: "local", label: "本机实时转录" },
  { value: "api", label: "API 转录" },
];

type ModelAvailability = { installed: boolean; selectable: boolean };

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function ModelDownloadProgress({ label, downloadedBytes, totalBytes }: {
  label: string;
  downloadedBytes: number;
  totalBytes: number;
}) {
  const downloaded = Number.isFinite(downloadedBytes) ? Math.max(0, downloadedBytes) : 0;
  const hasTotal = Number.isFinite(totalBytes) && totalBytes > 0;
  const percent = hasTotal ? Math.floor(Math.min(downloaded / totalBytes, 1) * 100) : undefined;

  return (
    <div className="meeting-speech-progress">
      <div className="meeting-speech-progress-track">
        <progress aria-label={label} max={100} value={percent} />
        {percent !== undefined && <span className="meeting-speech-progress-percent" aria-hidden="true">{percent}%</span>}
      </div>
      <p>
        已下载 {formatSize(downloaded)}
        {hasTotal && ` / ${formatSize(totalBytes)}`} · 可切换页面，下载将在后台继续。
      </p>
    </div>
  );
}

function ModelLocation({ path }: { path?: string }) {
  if (!path) return null;
  return <p className="meeting-speech-model-path"><span>存储目录</span><code>{path}</code></p>;
}

function ModelUninstall({ model, name, status, disabled, busy, onBusyChange, onUninstalled }: {
  model: LocalMeetingSpeechModelId | "speaker";
  name: string;
  status?: MeetingSpeechStatus;
  disabled: boolean;
  busy: boolean;
  onBusyChange: (busy: boolean) => void;
  onUninstalled: (status: MeetingSpeechStatus) => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState("");
  const inFlight = useRef(false);

  async function confirm() {
    if (disabled || !status?.canUninstall || inFlight.current) return;
    inFlight.current = true;
    setError("");
    onBusyChange(true);
    try {
      onUninstalled(await api.meetingSpeechUninstall(model));
      setConfirming(false);
    } catch (error) {
      setError(`卸载失败。${errorMessage(error)}`);
    } finally {
      inFlight.current = false;
      onBusyChange(false);
    }
  }

  function close() {
    if (!inFlight.current) setConfirming(false);
  }

  return (
    <>
      {status?.canUninstall && (
        <button type="button" disabled={disabled || busy} aria-label={`卸载 ${name}`} onClick={() => {
          setError("");
          setConfirming(true);
        }}>
          <Trash2 className="danger" size={15} /> 卸载
        </button>
      )}
      {confirming && (
        <Modal title={`卸载 ${name}？`} busy={busy} onClose={close}>
          <p>将删除此模型的本地文件。再次使用时可重新下载。</p>
          {error && <p className="error" role="alert">{error}</p>}
          <footer>
            <button type="button" disabled={busy} onClick={close}>取消</button>
            <button type="button" className="danger" disabled={busy || disabled || !status?.canUninstall}
              onClick={() => void confirm()}>
              {busy ? "正在卸载…" : "确认卸载"}
            </button>
          </footer>
        </Modal>
      )}
    </>
  );
}

function SpeakerModelDownload({ enabled }: { enabled: boolean }) {
  const [status, setStatus] = useState<MeetingSpeechStatus>();
  const [checkingError, setCheckingError] = useState("");
  const [installing, setInstalling] = useState(false);
  const [uninstalling, setUninstalling] = useState(false);
  const [installError, setInstallError] = useState("");
  const [checkVersion, setCheckVersion] = useState(0);

  useEffect(() => {
    if (uninstalling) return;
    let cancelled = false;
    let timer: number | undefined;

    async function refresh(keepPolling = false) {
      try {
        const next = await api.meetingSpeakerModelStatus();
        if (cancelled) return;
        setStatus(next);
        setCheckingError("");
        keepPolling = next.state === "downloading" || next.runtime === "loading";
      } catch (error) {
        if (cancelled) return;
        setCheckingError(errorMessage(error));
      }
      if (keepPolling) timer = window.setTimeout(() => void refresh(true), 1500);
    }

    void refresh();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [checkVersion, uninstalling]);

  async function install() {
    setInstalling(true);
    setInstallError("");
    try {
      await api.meetingSpeakerModelInstall();
      setStatus((current) => current && ({
        ...current,
        state: "downloading",
        canUninstall: false,
      }));
      setCheckVersion((value) => value + 1);
    } catch (error) {
      setInstallError(errorMessage(error));
    } finally {
      setInstalling(false);
    }
  }

  const loadingModel = status?.runtime === "loading";
  const ready = status?.state === "ready" && !checkingError && !loadingModel && !uninstalling;
  const canInstall = !checkingError && (status?.state === "missing" || status?.state === "error");
  const error = installError || checkingError || (status?.state === "error" && status.error);
  let label = modelStatusLabel(status);
  if (uninstalling) label = "正在卸载…";
  else if (installing) label = "正在启动下载…";
  else if (checkingError) label = "读取状态失败";
  else if (loadingModel) label = "已下载 · 正在加载…";
  else if (ready) label = "已下载 · 可识别说话人";

  return (
    <div className="meeting-speech-model">
      <div className="meeting-speech-model-heading">
        <span className="meeting-speech-model-icon"><Users size={20} /></span>
        <div className="meeting-speech-model-info">
          <h3>说话人模型 · Pyannote + CAM++</h3>
          <p>独立下载，用于本机录音时区分说话人，也可为已有录音补标。</p>
        </div>
        {checkingError && !installing && !uninstalling && (
          <button type="button" onClick={() => setCheckVersion((value) => value + 1)}>
            <RefreshCw size={15} /> 刷新状态
          </button>
        )}
        {canInstall && (
          <button type="button" className="primary" disabled={installing || uninstalling} onClick={() => void install()}>
            <Download size={15} /> {status?.state === "error" || installError ? "重新下载" : "下载说话人模型"}
          </button>
        )}
        <ModelUninstall
          model="speaker"
          name="Pyannote + CAM++"
          status={status}
          disabled={installing || loadingModel || !!checkingError || status?.state === "downloading"}
          busy={uninstalling}
          onBusyChange={setUninstalling}
          onUninstalled={(next) => {
            setInstallError("");
            setStatus(next);
          }}
        />
      </div>
      <ModelLocation path={status?.modelPath} />
      <p className="meeting-speech-status" data-state={ready ? "ready" : undefined} role="status">
        {ready && <CircleCheck size={15} />}
        {label}
        {status?.state === "missing" && ` · ${status.totalBytes > 0 ? formatSize(status.totalBytes) : "约 33 MiB"}`}
      </p>
      {status?.state === "downloading" && (
        <ModelDownloadProgress
          label="说话人模型下载进度"
          downloadedBytes={status.downloadedBytes}
          totalBytes={status.totalBytes}
        />
      )}
      {enabled && !ready && <p className="meeting-speech-resource-notice">可先保存设置。说话人模型就绪后才会区分说话人。</p>}
      {error && <p className="meeting-speech-error" role="alert">{error}</p>}
    </div>
  );
}

function TranscriptionModelDownload({ model, workspaceId, selected, recommended, onAvailabilityChange }: {
  model: LocalMeetingSpeechModelId;
  workspaceId: string;
  selected: boolean;
  recommended: boolean;
  onAvailabilityChange: (model: LocalMeetingSpeechModelId, installed: boolean, selectable: boolean) => void;
}) {
  const { name, devices, speed, accuracy, sizeNote } = localMeetingSpeechModels[model];
  const [status, setStatus] = useState<MeetingSpeechStatus>();
  const [checkingError, setCheckingError] = useState("");
  const [installError, setInstallError] = useState("");
  const [installing, setInstalling] = useState(false);
  const [uninstalling, setUninstalling] = useState(false);
  const [checkVersion, setCheckVersion] = useState(0);
  const [preparing, setPreparing] = useState(false);
  const [prepareError, setPrepareError] = useState("");
  const prepareInFlight = useRef(false);
  const autoPrepared = useRef(false);
  const installed = status?.state === "ready";
  const selectable = installed && !status.unsupportedReason && !checkingError && !installing && !uninstalling;

  useEffect(() => {
    onAvailabilityChange(model, installed, selectable);
  }, [model, installed, selectable, onAvailabilityChange]);

  useEffect(() => {
    if (!native || installing || uninstalling) return;
    let cancelled = false;
    let timer: number | undefined;

    async function refresh(keepPolling = false) {
      try {
        const next = await api.meetingSpeechStatus(model);
        if (cancelled) return;
        setStatus(next);
        setCheckingError("");
        keepPolling = next.state === "downloading" || next.runtime === "loading";
      } catch (error) {
        if (cancelled) return;
        setCheckingError(errorMessage(error));
      }
      if (keepPolling) timer = window.setTimeout(() => void refresh(true), 1500);
    }

    void refresh();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [checkVersion, installing, model, uninstalling]);

  const prepare = useCallback(async () => {
    if (prepareInFlight.current) return;
    prepareInFlight.current = true;
    setPreparing(true);
    setPrepareError("");
    try {
      await api.meetingSpeechPrepare(workspaceId);
    } catch (error) {
      setPrepareError(errorMessage(error));
    } finally {
      prepareInFlight.current = false;
      setPreparing(false);
      setCheckVersion((value) => value + 1);
    }
  }, [workspaceId]);

  useEffect(() => {
    if (!selected) {
      autoPrepared.current = false;
      return;
    }
    if (uninstalling || checkingError || status?.unsupportedReason
      || status?.state !== "ready" || status.runtime !== "cold" || autoPrepared.current) return;
    autoPrepared.current = true;
    void prepare();
  }, [checkingError, prepare, selected, status?.state, status?.runtime, status?.unsupportedReason, uninstalling]);

  const runtime = status?.state === "ready" ? status.runtime : undefined;
  const loadingModel = preparing || runtime === "loading";
  const runtimeError = !loadingModel && runtime !== "ready"
    ? (selected && prepareError) || (runtime === "error" && (status?.runtimeError || "本机模型加载失败，请重试。"))
    : "";
  const canInstall = !checkingError && (status?.state === "missing" || status?.state === "error");
  const canRetryPrepare = selected && status?.state === "ready" && !status.unsupportedReason
    && !uninstalling && !checkingError && !!runtimeError;
  const ready = !uninstalling && runtime === "ready" && !status?.unsupportedReason && !checkingError;
  const modelError = status?.unsupportedReason || installError || checkingError || (status?.state === "error" && status.error)
    || (status?.state === "ready" && runtimeError);

  async function install() {
    if (!canInstall || status?.unsupportedReason || installing || uninstalling) return;
    setInstalling(true);
    setInstallError("");
    try {
      await api.meetingSpeechInstall(model);
      setStatus((current) => current && ({
        ...current,
        state: "downloading",
        downloadedBytes: 0,
        canUninstall: false,
      }));
    } catch (error) {
      setInstallError(errorMessage(error));
    } finally {
      setInstalling(false);
    }
  }

  let label = modelStatusLabel(status);
  if (!native) label = "请在桌面应用中使用";
  else if (status?.unsupportedReason) label = "当前设备不支持";
  else if (uninstalling) label = "正在卸载…";
  else if (installing) label = "正在启动下载…";
  else if (checkingError) label = "读取状态失败";
  else if (status?.state === "ready") {
    if (loadingModel) label = "已安装 · 正在预加载…";
    else if (runtime === "ready") label = selected ? "已加载 · 可实时转录" : "已加载";
    else if (runtimeError) label = "已安装 · 加载失败";
    else if (selected && runtime === "cold") label = "已安装 · 等待加载";
    else label = "已安装";
  }

  return (
    <div className="meeting-speech-model" data-model={model}>
      <div className="meeting-speech-model-heading">
        <span className="meeting-speech-model-icon"><AudioLines size={20} /></span>
        <div className="meeting-speech-model-info">
          <h3>{name} {recommended && <span className="meeting-speech-recommended">推荐</span>}</h3>
          {selected && <span className="meeting-speech-saved"><CircleCheck size={14} /> 已保存的转录模型</span>}
        </div>
        {!uninstalling && !installing && checkingError && (
          <button type="button" onClick={() => setCheckVersion((value) => value + 1)}>
            <RefreshCw size={15} /> 刷新状态
          </button>
        )}
        {canInstall && (
          <button type="button" className="primary" aria-label={`下载 ${name}`}
            disabled={installing || uninstalling || !!status?.unsupportedReason} onClick={() => void install()}>
            <Download size={15} /> {status?.state === "error" ? "重新下载" : "下载模型"}
          </button>
        )}
        {canRetryPrepare && (
          <button type="button" onClick={() => void prepare()}>
            <RefreshCw size={15} /> 重试加载
          </button>
        )}
        {native && (
          <ModelUninstall
            model={model}
            name={name}
            status={status}
            disabled={installing || loadingModel || !!checkingError || status?.state === "downloading"}
            busy={uninstalling}
            onBusyChange={setUninstalling}
            onUninstalled={(next) => {
              autoPrepared.current = false;
              setPrepareError("");
              setInstallError("");
              setStatus(next);
            }}
          />
        )}
      </div>
      <div className="meeting-speech-model-details">
        <p>{devices}</p>
        <p>{speed}</p>
        <p>{accuracy}</p>
        {sizeNote && <p>{sizeNote}</p>}
      </div>
      <ModelLocation path={status?.modelPath} />
      <p className="meeting-speech-status" data-state={ready ? "ready" : undefined} role="status">
        {ready && <CircleCheck size={15} />}
        {label}
        {!!status?.totalBytes && ` · 下载总量 ${formatSize(status.totalBytes)}`}
      </p>
      {status?.state === "downloading" && (
        <ModelDownloadProgress
          label={`${name} 下载进度`}
          downloadedBytes={status.downloadedBytes}
          totalBytes={status.totalBytes}
        />
      )}
      {modelError && <p className="meeting-speech-error" role="alert">{modelError}</p>}
    </div>
  );
}

export default function MeetingSpeechSettings({ data, workspaceId, onConfigureProvider }: Props) {
  const [mode, setMode] = useState<MeetingSpeechPreferences["mode"]>("local");
  const [savedPreferences, setSavedPreferences] = useState<MeetingSpeechPreferences>();
  const [modelId, setModelId] = useState("");
  const [localModelId, setLocalModelId] = useState<LocalMeetingSpeechModelId>(defaultLocalMeetingSpeechModelId);
  const [modelAvailability, setModelAvailability] = useState<Partial<Record<LocalMeetingSpeechModelId, ModelAvailability>>>({});
  const [recommendation, setRecommendation] = useState<MeetingSpeechRecommendation>();
  const [localSpeakerDiarization, setLocalSpeakerDiarization] = useState(false);
  const [inputDeviceId, setInputDeviceId] = useState("");
  const [loadError, setLoadError] = useState("");
  const [loadVersion, setLoadVersion] = useState(0);
  const [saveError, setSaveError] = useState("");
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const saveInFlight = useRef(false);

  const updateModelAvailability = useCallback((model: LocalMeetingSpeechModelId, installed: boolean, selectable: boolean) => {
    setModelAvailability((current) => {
      const previous = current[model];
      return previous?.installed === installed && previous.selectable === selectable
        ? current : { ...current, [model]: { installed, selectable } };
    });
  }, []);

  useEffect(() => {
    if (!native) return;
    let cancelled = false;
    void api.meetingSpeechPreferences(workspaceId).then((value) => {
      if (cancelled) return;
      setSavedPreferences(value);
      setMode(value.mode);
      setLocalModelId(value.localModelId ?? defaultLocalMeetingSpeechModelId);
      setModelId(value.mode === "api" ? value.modelId : "");
      setLocalSpeakerDiarization(value.mode === "local" && value.localSpeakerDiarization === true);
      setInputDeviceId(value.inputDeviceId ?? "");
    }).catch((error) => {
      if (!cancelled) setLoadError(errorMessage(error));
    });
    return () => { cancelled = true; };
  }, [workspaceId, loadVersion]);

  useEffect(() => {
    if (!native) return;
    let cancelled = false;
    void api.meetingSpeechRecommendation().then((value) => {
      if (!cancelled && localMeetingSpeechModelIds.includes(value.model)) {
        setRecommendation(value);
      }
    }).catch(() => {});
    return () => { cancelled = true; };
  }, []);

  const speechModels = eligibleMeetingSpeechModels(data);
  const selectedModel = speechModels.find((model) => model.id === modelId);
  const selectedProvider = data.providers.find((provider) => provider.id === selectedModel?.providerId);
  const modelOptions = speechModels.map((model) => {
    const providerName = data.providers.find((provider) => provider.id === model.providerId)?.name;
    return { value: model.id, label: providerName ? `${model.name} · ${providerName}` : model.name };
  });
  const loaded = !!savedPreferences;
  const savedLocalModelId = savedPreferences?.mode === "local"
    ? savedPreferences.localModelId ?? defaultLocalMeetingSpeechModelId
    : undefined;
  const localModelOptions = localMeetingSpeechModelIds.map((value) => ({
    value,
    label: localMeetingSpeechModels[value].name,
    disabled: !modelAvailability[value]?.selectable,
  }));
  const recommendedModelId = recommendation?.model
    ?? localMeetingSpeechModelIds.find((id) => localMeetingSpeechModels[id].recommended)
    ?? "r2t2";
  const modelRank = (model: LocalMeetingSpeechModelId) =>
    model === recommendedModelId ? 0 : modelAvailability[model]?.installed ? 1 : 2;
  const orderedModels = [...localMeetingSpeechModelIds].sort((a, b) => modelRank(a) - modelRank(b));
  const canSave = loaded && (mode === "local"
    ? !!modelAvailability[localModelId]?.selectable || localModelId === savedLocalModelId
    : !!selectedModel);

  function clearSaveResult() {
    setSaved(false);
    setSaveError("");
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canSave || saveInFlight.current) return;
    const devicePreference = inputDeviceId ? { inputDeviceId } : {};
    const savedModel = savedPreferences?.localModelId;
    const value: MeetingSpeechPreferences = mode === "api"
      ? { mode: "api", modelId, ...(savedModel ? { localModelId: savedModel } : {}), ...devicePreference }
      : { mode: "local", localModelId, localSpeakerDiarization, ...devicePreference };
    saveInFlight.current = true;
    setSaving(true);
    clearSaveResult();
    try {
      await api.saveMeetingSpeechPreferences(workspaceId, value);
      setSavedPreferences(value);
      setSaved(true);
    } catch (error) {
      setSaveError(errorMessage(error));
    } finally {
      saveInFlight.current = false;
      setSaving(false);
    }
  }

  return (
    <section className="meeting-speech-settings" aria-labelledby="meeting-speech-heading">
      <h2 id="meeting-speech-heading">会议语音</h2>
      {!native && <p>请在桌面应用中保存会议语音设置。</p>}
      {native && !loaded && (
        loadError
          ? <div className="meeting-speech-load-error">
              <p className="meeting-speech-error" role="alert">无法读取会议语音设置。{loadError}</p>
              <button type="button" onClick={() => {
                setLoadError("");
                setLoadVersion((value) => value + 1);
              }}><RefreshCw size={15} /> 重试</button>
            </div>
          : <p role="status">正在读取会议语音设置…</p>
      )}
      {loaded && (
        <form className="meeting-speech-form" onSubmit={save}>
          <MeetingMicrophoneSettings
            deviceId={inputDeviceId}
            disabled={saving}
            onDeviceChange={(deviceId) => {
              setInputDeviceId(deviceId);
              clearSaveResult();
            }}
          />
          <div className="meeting-speech-field">
            <label htmlFor="meeting-speech-mode">转录方式</label>
            <GlassSelect
              id="meeting-speech-mode"
              value={mode}
              options={modeOptions}
              disabled={saving}
              onValueChange={(value) => {
                if (value !== "local" && value !== "api") return;
                setMode(value);
                clearSaveResult();
              }}
            />
          </div>
          {mode === "local" ? (
            <div className="meeting-speech-local">
              <div className="meeting-speech-field">
                <label htmlFor="meeting-speech-local-model">本机语音模型</label>
                <GlassSelect
                  id="meeting-speech-local-model"
                  value={localModelId}
                  options={localModelOptions}
                  disabled={saving}
                  aria-describedby="meeting-speech-local-help"
                  onValueChange={(value) => {
                    const model = localMeetingSpeechModelIds.find((id) => id === value);
                    if (!model || !modelAvailability[model]?.selectable) return;
                    setLocalModelId(model);
                    clearSaveResult();
                  }}
                />
              </div>
              <p id="meeting-speech-local-help">请先在下方下载模型，再选择并保存。新录音将使用所选模型离线转录。</p>
              <label className="meeting-speech-speaker-toggle">
                <input type="checkbox" checked={localSpeakerDiarization} disabled={saving}
                  onChange={(event) => {
                    setLocalSpeakerDiarization(event.target.checked);
                    clearSaveResult();
                  }} />
                本机识别说话人
              </label>
              {localSpeakerDiarization && (
                <p>
                  {(localModelId === "r2t2" || localModelId === "qwen3-asr-0.6b") && "大模型流式转录按整段确认说话人。"}
                  较短、重叠或暂时无法确认的发言可能先显示为未标记。已有录音可手动点击「识别说话人」补标，说话人标签可以改名。
                </p>
              )}
              <p>模型文件不随工作空间备份。</p>
            </div>
          ) : (
            <div className="meeting-speech-api">
              <div className="meeting-speech-field">
                <label htmlFor="meeting-speech-api-model">API 语音模型</label>
                <GlassSelect
                  id="meeting-speech-api-model"
                  value={selectedModel?.id ?? ""}
                  options={modelOptions}
                  placeholder={modelOptions.length ? "请选择语音模型" : "没有可用的语音模型"}
                  disabled={saving || !modelOptions.length}
                  aria-describedby="meeting-speech-api-help"
                  onValueChange={(value) => {
                    setModelId(value);
                    clearSaveResult();
                  }}
                />
              </div>
              {modelId && !selectedModel && (
                <p className="meeting-speech-error" role="alert">原选模型已不可用，请重新选择。</p>
              )}
              <p id="meeting-speech-api-help">
                录音片段会发送到所选模型对应的 API。供应商需支持 OpenAI 兼容的语音转录接口。
                拉取或手动添加模型时，请将模型类型设为「语音」并启用。
              </p>
              <div className="meeting-speech-actions">
                {onConfigureProvider && selectedProvider && (
                  <button type="button" disabled={saving} onClick={() => onConfigureProvider(selectedProvider)}>
                    <Settings2 size={15} /> 配置供应商
                  </button>
                )}
                {onConfigureProvider && (
                  <button type="button" disabled={saving} onClick={() => onConfigureProvider()}>
                    <Plus size={15} /> 添加供应商
                  </button>
                )}
                <Link to="?tab=models">前往大模型管理</Link>
              </div>
            </div>
          )}
          {saveError && <p className="meeting-speech-error" role="alert">{saveError}</p>}
          <div className="meeting-speech-actions">
            <button type="submit" className="primary" disabled={saving || !canSave}>
              {saving ? "正在保存…" : "保存设置"}
            </button>
            {saved && <span className="meeting-speech-saved" role="status"><CircleCheck size={15} /> 设置已保存</span>}
          </div>
        </form>
      )}
      {recommendation && (
        <div className="meeting-speech-recommendation" role="status">
          <span className="meeting-speech-model-icon"><Cpu size={18} /></span>
          <div className="meeting-speech-recommendation-body">
            <div className="meeting-speech-recommendation-title">
              <strong>本机配置推荐：{localMeetingSpeechModels[recommendation.model].name}</strong>
              <span className="meeting-speech-device-pill">{recommendation.deviceSummary}</span>
            </div>
            <p>{recommendation.reason}</p>
          </div>
        </div>
      )}
      {orderedModels.map((model) => (
        <TranscriptionModelDownload key={model} model={model} workspaceId={workspaceId}
          selected={savedLocalModelId === model} recommended={recommendedModelId === model}
          onAvailabilityChange={updateModelAvailability} />
      ))}
      {native && <SpeakerModelDownload enabled={loaded && mode === "local" && localSpeakerDiarization} />}
    </section>
  );
}
