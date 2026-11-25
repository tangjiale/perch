import type { Snapshot } from "./types";

export const localMeetingSpeechModelIds = [
  "r2t2",
  "qwen3-asr-0.6b",
  "transcription",
  "x-asr-480",
  "zipformer-zh",
] as const;
export type LocalMeetingSpeechModelId = typeof localMeetingSpeechModelIds[number];
export const defaultLocalMeetingSpeechModelId: LocalMeetingSpeechModelId = "transcription";

export type MeetingSpeechRecommendation = {
  model: LocalMeetingSpeechModelId;
  deviceSummary: string;
  reason: string;
  cpuCores: number;
  memoryGib?: number;
  accelerator: "metal" | "cpu";
};

export const localMeetingSpeechModels: Record<LocalMeetingSpeechModelId, {
  name: string;
  devices: string;
  speed: string;
  accuracy: string;
  sizeNote?: string;
  recommended?: boolean;
}> = {
  r2t2: {
    name: "有道 Confucius4-R2T2",
    devices: "Apple 芯片 Mac · Metal / Windows x64 · CPU",
    speed: "按 320 ms 音频块处理，出字速度取决于设备性能和语音内容。",
    accuracy: "支持中英文流式转录。有道公开会议评测的字错误率为 6.74%，与本机量化版的实际结果可能不同。",
    sizeNote: "模型权重约 1.19 GB，另需下载运行包。",
    recommended: true,
  },
  "qwen3-asr-0.6b": {
    name: "Qwen3-ASR-0.6B",
    devices: "Apple 芯片 Mac · Metal / Windows x64 · CPU",
    speed: "按约 4 秒音频窗口流式输出，0.6B 参数量在 CPU 与 8 GB 内存设备上更轻快。",
    accuracy: "支持 30 种语言与 22 种中文方言识别，适合中英混说、方言及复杂会议场景。",
    sizeNote: "模型权重约 1.15 GB（Q8_0），另需下载运行包。",
  },
  transcription: {
    name: "X-ASR 160 ms",
    devices: "macOS / Windows · INT8，CPU 推理",
    speed: "按 160 ms 音频块处理，适合优先考虑出字速度的场景。",
    accuracy: "支持中文、英文和混说，可作为日常会议的默认选项。",
  },
  "x-asr-480": {
    name: "X-ASR 480 ms",
    devices: "macOS / Windows · INT8，CPU 推理",
    speed: "按 480 ms 音频块处理，出字等待通常更长。",
    accuracy: "多人背景实测中文字错率（CER）从 18.38% 降到 14.44%，会议短片中的准确率仍随样本波动。",
  },
  "zipformer-zh": {
    name: "Zipformer 中文",
    devices: "macOS / Windows · CPU 推理",
    speed: "中文首字快，适合优先查看实时文字。",
    accuracy: "已测普通话样本的准确率低于 X-ASR，含专有名词的场景需实测。",
  },
};

export type MeetingSpeechPreferences = (
  | { mode: "local" }
  | { mode: "api"; modelId: string }
) & {
  inputDeviceId?: string;
  localModelId?: LocalMeetingSpeechModelId;
  localSpeakerDiarization?: boolean;
};

export function eligibleMeetingSpeechModels({ models, providers }: Snapshot) {
  const enabledProviders = new Set(
    providers
      .filter((provider) => provider.enabled && provider.protocol !== "anthropic-messages")
      .map((provider) => provider.id),
  );
  return models.filter(
    (model) => model.enabled && model.capability === "speech" && enabledProviders.has(model.providerId),
  );
}
