import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { Link } from "react-router-dom";
import { AudioLines, CircleCheck, Download, Plus, RefreshCw, Settings2, Trash2, Users } from "lucide-react";
import GlassSelect from "../../components/GlassSelect";
import Modal from "../../components/Modal";
import { api, native, type MeetingSpeechStatus } from "../../lib/api";
import { eligibleMeetingSpeechModels, type MeetingSpeechPreferences } from "../../lib/meeting-speech";
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
  return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
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

const localModelOptions = [
  { value: "zipformer", label: "X-ASR（实时转录）" },
  { value: "zipformer-speakers", label: "X-ASR + 说话人识别" },
];

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
  model: "transcription" | "speaker";
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
          <Trash2 size={15} /> 卸载
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
          <p>与 X-ASR 分开下载，用于本机录音时区分说话人，也可为已有录音补标。</p>
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

export default function MeetingSpeechSettings({ data, workspaceId, onConfigureProvider }: Props) {
  const [mode, setMode] = useState<MeetingSpeechPreferences["mode"]>("local");
  const [savedMode, setSavedMode] = useState<MeetingSpeechPreferences["mode"]>();
  const [modelId, setModelId] = useState("");
  const [localSpeakerDiarization, setLocalSpeakerDiarization] = useState(false);
  const [inputDeviceId, setInputDeviceId] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState("");
  const [loadVersion, setLoadVersion] = useState(0);
  const [saveError, setSaveError] = useState("");
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const saveInFlight = useRef(false);
  const [status, setStatus] = useState<MeetingSpeechStatus>();
  const [checkingError, setCheckingError] = useState("");
  const [installError, setInstallError] = useState("");
  const [installing, setInstalling] = useState(false);
  const [uninstalling, setUninstalling] = useState(false);
  const [checkVersion, setCheckVersion] = useState(0);
  const [preparing, setPreparing] = useState(false);
  const [prepareError, setPrepareError] = useState("");
  const prepareInFlight = useRef(false);
  const autoPrepareWorkspace = useRef<string | undefined>(undefined);

  useEffect(() => {
    if (!native) return;
    let cancelled = false;
    void api.meetingSpeechPreferences(workspaceId).then((value) => {
      if (cancelled) return;
      setMode(value.mode);
      setSavedMode(value.mode);
      setModelId(value.mode === "api" ? value.modelId : "");
      setLocalSpeakerDiarization(value.mode === "local" && value.localSpeakerDiarization === true);
      setInputDeviceId(value.inputDeviceId ?? "");
      setLoaded(true);
    }).catch((error) => {
      if (!cancelled) setLoadError(errorMessage(error));
    });
    return () => { cancelled = true; };
  }, [workspaceId, loadVersion]);

  useEffect(() => {
    if (!native || uninstalling) return;
    let cancelled = false;
    let timer: number | undefined;

    async function refresh(keepPolling = false) {
      try {
        const next = await api.meetingSpeechStatus();
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
    if (uninstalling || checkingError || savedMode !== "local" || status?.state !== "ready" || status.runtime !== "cold"
      || autoPrepareWorkspace.current === workspaceId) return;
    autoPrepareWorkspace.current = workspaceId;
    void prepare();
  }, [checkingError, prepare, savedMode, status?.state, status?.runtime, uninstalling, workspaceId]);

  const speechModels = eligibleMeetingSpeechModels(data);
  const selectedModel = speechModels.find((model) => model.id === modelId);
  const selectedProvider = data.providers.find((provider) => provider.id === selectedModel?.providerId);
  const modelOptions = speechModels.map((model) => {
    const providerName = data.providers.find((provider) => provider.id === model.providerId)?.name;
    return { value: model.id, label: providerName ? `${model.name} · ${providerName}` : model.name };
  });
  const canSave = loaded && (mode === "local" || !!selectedModel);

  function clearSaveResult() {
    setSaved(false);
    setSaveError("");
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canSave || saveInFlight.current) return;
    const devicePreference = inputDeviceId ? { inputDeviceId } : {};
    const value: MeetingSpeechPreferences = mode === "api"
      ? { mode: "api", modelId, ...devicePreference }
      : { mode: "local", localSpeakerDiarization, ...devicePreference };
    saveInFlight.current = true;
    setSaving(true);
    clearSaveResult();
    try {
      await api.saveMeetingSpeechPreferences(workspaceId, value);
      setSavedMode(value.mode);
      setCheckVersion((revision) => revision + 1);
      setSaved(true);
    } catch (error) {
      setSaveError(errorMessage(error));
    } finally {
      saveInFlight.current = false;
      setSaving(false);
    }
  }

  async function install() {
    setInstalling(true);
    setInstallError("");
    try {
      await api.meetingSpeechInstall();
      setStatus((current) => current && ({
        ...current,
        state: "downloading",
        downloadedBytes: 0,
        canUninstall: false,
      }));
      setCheckVersion((value) => value + 1);
    } catch (error) {
      setInstallError(errorMessage(error));
    } finally {
      setInstalling(false);
    }
  }

  const runtime = status?.state === "ready" ? status.runtime : undefined;
  const loadingModel = preparing || runtime === "loading";
  const runtimeError = !loadingModel && runtime !== "ready"
    ? prepareError || (runtime === "error" && (status?.runtimeError || "本机模型加载失败，请重试。"))
    : "";
  let label = modelStatusLabel(status);
  if (!native) label = "请在桌面应用中使用";
  else if (uninstalling) label = "正在卸载…";
  else if (installing) label = "正在启动下载…";
  else if (checkingError && !status) label = "读取状态失败";
  else if (status?.state === "ready") {
    if (loadingModel) label = "已下载 · 正在预加载…";
    else if (runtime === "ready") label = "已加载 · 可实时转录";
    else if (runtimeError) label = "已下载 · 加载失败";
    else if (savedMode === "api" && mode === "local") label = "已下载 · 保存后自动加载";
    else if (runtime === "cold") label = "已下载 · 等待加载";
  }
  const modelError = installError || checkingError || (status?.state === "error" && status.error)
    || (status?.state === "ready" && runtimeError);
  const canRetryPrepare = savedMode === "local" && mode === "local" && status?.state === "ready"
    && !uninstalling && !checkingError && !!runtimeError;
  const canInstall = !checkingError && (status?.state === "missing" || status?.state === "error");

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
                  value={localSpeakerDiarization ? "zipformer-speakers" : "zipformer"}
                  options={localModelOptions}
                  disabled={saving}
                  aria-describedby="meeting-speech-local-help"
                  onValueChange={(value) => {
                    setLocalSpeakerDiarization(value === "zipformer-speakers");
                    clearSaveResult();
                  }}
                />
              </div>
              <p id="meeting-speech-local-help">
                {localSpeakerDiarization
                  ? "录音时实时显示文字并在本机识别说话人。较短、重叠或暂时无法确认的发言可能先显示为未标记。已有录音可手动点击「识别说话人」补标，说话人标签可以改名。"
                  : "录音时实时显示文字。"}
              </p>
              <p>模型下载后可离线使用，模型文件不随工作空间备份。</p>
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
      <div className="meeting-speech-model">
        <div className="meeting-speech-model-heading">
          <span className="meeting-speech-model-icon"><AudioLines size={20} /></span>
          <div className="meeting-speech-model-info">
            <h3>本机模型 · X-ASR 160 ms（中英双语，INT8）</h3>
            <p>下载约 128 MiB。模型按 160 ms 的音频块处理，实际出字时间取决于设备性能和语音内容。</p>
          </div>
          {!uninstalling && !installing && checkingError && status?.state !== "downloading" && (
            <button type="button" onClick={() => setCheckVersion((value) => value + 1)}>
              <RefreshCw size={15} /> 刷新状态
            </button>
          )}
          {native && canInstall && (
            <button type="button" className="primary" disabled={installing || uninstalling} onClick={() => void install()}>
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
              model="transcription"
              name="X-ASR"
              status={status}
              disabled={installing || loadingModel || !!checkingError || status?.state === "downloading"}
              busy={uninstalling}
              onBusyChange={setUninstalling}
              onUninstalled={(next) => {
                autoPrepareWorkspace.current = undefined;
                setPrepareError("");
                setInstallError("");
                setStatus(next);
              }}
            />
          )}
        </div>
        <ModelLocation path={status?.modelPath} />
        <p className="meeting-speech-status" data-state={!uninstalling && runtime === "ready" ? "ready" : undefined} role="status">
          {!uninstalling && runtime === "ready" && <CircleCheck size={15} />}
          {label}
          {status?.state === "missing" && status.totalBytes > 0 && ` · ${formatSize(status.totalBytes)}`}
        </p>
        {status?.state === "downloading" && (
          <ModelDownloadProgress
            label="会议语音模型下载进度"
            downloadedBytes={status.downloadedBytes}
            totalBytes={status.totalBytes}
          />
        )}
        {modelError && <p className="meeting-speech-error" role="alert">{modelError}</p>}
      </div>
      {native && <SpeakerModelDownload enabled={loaded && mode === "local" && localSpeakerDiarization} />}
    </section>
  );
}
