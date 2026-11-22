import { useCallback, useEffect, useRef, useState } from "react";
import { Mic, MicOff } from "lucide-react";
import GlassSelect from "../../components/GlassSelect";
import { meetingAudioConstraints } from "../../lib/meeting-recorder";

const meterSegments = 12;

interface Props {
  deviceId: string;
  onDeviceChange: (deviceId: string) => void;
  disabled: boolean;
}

interface MicrophoneCheck {
  deviceId: string;
  onEnded: () => void;
  stream?: MediaStream;
  context?: AudioContext;
  source?: MediaStreamAudioSourceNode;
  timer?: number;
}

function stopStream(stream: MediaStream) {
  for (const track of stream.getTracks()) track.stop();
}

function checkErrorMessage(error: unknown) {
  switch (error instanceof DOMException || error instanceof Error ? error.name : "") {
    case "NotAllowedError":
    case "SecurityError":
      return "无法访问麦克风，请在系统设置中允许栖点使用麦克风后重试。";
    case "NotFoundError":
    case "OverconstrainedError":
      return "找不到所选麦克风，请检查设备连接或选择其他设备。";
    case "NotReadableError":
      return "无法启动麦克风，请检查设备是否被其他应用占用后重试。";
    default:
      return "麦克风检测启动失败，请检查设备和权限后重试。";
  }
}

export default function MeetingMicrophoneSettings({ deviceId, onDeviceChange, disabled }: Props) {
  const mediaDevices = navigator.mediaDevices;
  const canCheck = !!mediaDevices?.getUserMedia && typeof AudioContext !== "undefined";
  const [inputs, setInputs] = useState<MediaDeviceInfo[]>([]);
  const [listError, setListError] = useState("");
  const [state, setState] = useState<"idle" | "starting" | "active">("idle");
  const [error, setError] = useState("");
  const [defaultLabel, setDefaultLabel] = useState("");
  const [level, setLevel] = useState(0);
  const activeCheck = useRef<MicrophoneCheck | undefined>(undefined);
  const enumeration = useRef(0);
  const knownLabels = useRef(new Map<string, string>());

  const releaseCheck = useCallback(() => {
    const check = activeCheck.current;
    activeCheck.current = undefined;
    if (!check) return;
    window.clearInterval(check.timer);
    check.source?.disconnect();
    if (check.stream) {
      for (const track of check.stream.getAudioTracks()) {
        track.removeEventListener("ended", check.onEnded);
      }
      stopStream(check.stream);
    }
    if (check.context && check.context.state !== "closed") {
      void check.context.close().catch(() => undefined);
    }
  }, []);

  const stopCheck = useCallback((message = "") => {
    releaseCheck();
    setState("idle");
    setError(message);
    setDefaultLabel("");
    setLevel(0);
  }, [releaseCheck]);

  const refreshDevices = useCallback(async () => {
    if (!mediaDevices?.enumerateDevices || document.hidden) return;
    const request = ++enumeration.current;
    try {
      const devices = await mediaDevices.enumerateDevices();
      if (request !== enumeration.current) return;
      const next = devices.filter((device) => device.kind === "audioinput");
      for (const device of next) {
        if (device.deviceId && device.label) knownLabels.current.set(device.deviceId, device.label);
      }
      setInputs(next);
      setListError("");
      const check = activeCheck.current;
      const activeDeviceId = check?.deviceId || check?.stream?.getAudioTracks()[0]?.getSettings().deviceId;
      if (check?.stream && activeDeviceId && activeDeviceId !== "default"
        && next.every((device) => device.deviceId) && !next.some((device) => device.deviceId === activeDeviceId)) {
        stopCheck("麦克风已断开，检测已结束。");
      }
    } catch {
      if (request === enumeration.current) setListError("无法读取麦克风列表，请检查设备连接和系统权限。");
    }
  }, [mediaDevices, stopCheck]);

  useEffect(() => {
    function onDeviceChange() { void refreshDevices(); }
    function onPageHide() {
      enumeration.current++;
      stopCheck();
    }
    function onVisibilityChange() {
      if (document.hidden) onPageHide();
      else onDeviceChange();
    }

    void refreshDevices();
    mediaDevices?.addEventListener("devicechange", onDeviceChange);
    document.addEventListener("visibilitychange", onVisibilityChange);
    window.addEventListener("pagehide", onPageHide);
    return () => {
      enumeration.current++;
      releaseCheck();
      mediaDevices?.removeEventListener("devicechange", onDeviceChange);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.removeEventListener("pagehide", onPageHide);
    };
  }, [mediaDevices, refreshDevices, releaseCheck, stopCheck]);

  async function startCheck(nextDeviceId: string) {
    stopCheck();
    if (!canCheck || document.hidden) return;
    const check: MicrophoneCheck = {
      deviceId: nextDeviceId,
      onEnded: () => {
        if (activeCheck.current !== check) return;
        stopCheck("麦克风已断开或权限已撤回，检测已结束。");
        void refreshDevices();
      },
    };
    activeCheck.current = check;
    setState("starting");
    try {
      const context = new AudioContext();
      check.context = context;
      const capture = mediaDevices.getUserMedia(meetingAudioConstraints(nextDeviceId || undefined)).then((stream) => {
        if (activeCheck.current !== check) {
          stopStream(stream);
          return;
        }
        check.stream = stream;
        if (document.hidden) {
          stopCheck();
          return;
        }
        const track = stream.getAudioTracks()[0];
        if (!track || track.readyState === "ended") {
          check.onEnded();
          return;
        }
        track.addEventListener("ended", check.onEnded);
        if (!nextDeviceId) setDefaultLabel(track.label);
        void refreshDevices();
        return stream;
      });
      // 在启动检测的点击中恢复音频上下文，授权对话框可能延后麦克风流返回。
      const [stream] = await Promise.all([capture, context.resume()]);
      if (!stream || activeCheck.current !== check) return;
      const source = context.createMediaStreamSource(stream);
      check.source = source;
      const analyser = context.createAnalyser();
      analyser.fftSize = 1024;
      source.connect(analyser);
      const samples = new Float32Array(analyser.fftSize);
      let displayed = 0;
      check.timer = window.setInterval(() => {
        analyser.getFloatTimeDomainData(samples);
        let energy = 0;
        for (const sample of samples) energy += sample * sample;
        const rms = Math.sqrt(energy / samples.length);
        const decibels = 20 * Math.log10(Math.max(rms, 0.00001));
        const normalized = Math.min(1, Math.max(0, (decibels + 55) / 55));
        displayed = Math.max(normalized, displayed * 0.72);
        setLevel(Math.round(displayed * meterSegments));
      }, 65);
      setState("active");
    } catch (cause) {
      if (activeCheck.current === check) {
        stopCheck(checkErrorMessage(cause));
        void refreshDevices();
      }
    }
  }

  const systemLabel = inputs.find((input) => input.deviceId === "default")?.label || defaultLabel;
  const physicalInputs = inputs.filter((input) => input.deviceId && input.deviceId !== "default");
  const unknownDevice = !!deviceId && !physicalInputs.some((input) => input.deviceId === deviceId);
  const missingDevice = unknownDevice && (inputs.length > 0 || knownLabels.current.has(deviceId))
    && inputs.every((input) => !!input.deviceId);
  const options = [
    { value: "", label: systemLabel ? `系统默认（${systemLabel}）` : "系统默认" },
    ...physicalInputs.map((input, index) => ({ value: input.deviceId, label: input.label || `麦克风 ${index + 1}` })),
  ];
  if (unknownDevice) {
    const name = knownLabels.current.get(deviceId) || "已选麦克风";
    options.push({ value: deviceId, label: `${name}（${missingDevice ? "不可用" : "待确认"}）` });
  }

  return (
    <div className="meeting-microphone-settings">
      <div className="meeting-speech-field meeting-microphone-field">
        <label htmlFor="meeting-microphone-device">输入设备</label>
        <GlassSelect
          id="meeting-microphone-device"
          icon={<Mic size={15} />}
          value={deviceId}
          options={options}
          disabled={disabled}
          aria-describedby="meeting-microphone-help"
          onValueChange={(nextDeviceId) => {
            const wasChecking = !!activeCheck.current;
            onDeviceChange(nextDeviceId);
            if (wasChecking) void startCheck(nextDeviceId);
            else stopCheck();
          }}
        />
      </div>
      <div className="meeting-microphone-check">
        <div
          className="meeting-microphone-meter"
          role="meter"
          aria-label="麦克风输入音量"
          aria-valuemin={0}
          aria-valuemax={meterSegments}
          aria-valuenow={level}
        >
          {Array.from({ length: meterSegments }, (_, index) => (
            <span key={index} data-lit={index < level || undefined} aria-hidden="true" />
          ))}
        </div>
        <button
          type="button"
          disabled={state === "idle" && (disabled || !canCheck || missingDevice)}
          onClick={() => {
            if (activeCheck.current) stopCheck();
            else void startCheck(deviceId);
          }}
        >
          {state === "idle" ? <Mic size={15} aria-hidden="true" /> : <MicOff size={15} aria-hidden="true" />}
          {state === "idle" ? "检测麦克风" : "结束检测"}
        </button>
      </div>
      <p id="meeting-microphone-help" className="meeting-microphone-message" role="status">
        {state === "starting" ? "正在打开麦克风…"
          : state === "active" ? "请对麦克风说话，观察音量条。"
            : !canCheck ? "当前环境无法检测麦克风。"
              : missingDevice ? "所选麦克风不可用，重新连接或选择其他设备。"
                : "首次检测需授权麦克风。检测音频不会播放或保存。"}
      </p>
      {(error || listError) && <p className="meeting-speech-error meeting-microphone-message" role="alert">{error || listError}</p>}
    </div>
  );
}
