/** 每个分片都是完整的 16kHz 单声道 PCM WAV，可独立转录和重建录音。 */
export function pcmWav(samples: Float32Array, sourceRate: number): Uint8Array {
  const actualSize = Math.floor((samples.length * 16000) / sourceRate),
    size = Math.ceil(actualSize / 16) * 16,
    bytes = new Uint8Array(44 + size * 2),
    view = new DataView(bytes.buffer);
  const word = (offset: number, value: string) =>
    [...value].forEach((c, i) => view.setUint8(offset + i, c.charCodeAt(0)));
  word(0, "RIFF");
  view.setUint32(4, bytes.length - 8, true);
  word(8, "WAVE");
  word(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, 16000, true);
  view.setUint32(28, 32000, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  word(36, "data");
  view.setUint32(40, size * 2, true);
  // 区间均值在降采样时降低混叠，避免简单丢弃高采样率输入点。
  for (let i = 0; i < actualSize; i++) {
    const begin = Math.floor((i * sourceRate) / 16000),
      end = Math.max(begin + 1, Math.floor(((i + 1) * sourceRate) / 16000));
    let sum = 0;
    for (let j = begin; j < end; j++) sum += samples[j] || 0;
    const s = Math.max(-1, Math.min(1, sum / (end - begin)));
    view.setInt16(44 + i * 2, s < 0 ? s * 32768 : s * 32767, true);
  }
  return bytes;
}
export function bytesBase64(bytes: Uint8Array): string {
  let text = "";
  for (let i = 0; i < bytes.length; i += 8192)
    text += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return btoa(text);
}
export class MeetingRecorder {
  private stream?: MediaStream;
  private context?: AudioContext;
  private processor?: ScriptProcessorNode;
  private source?: MediaStreamAudioSourceNode;
  private chunks: Float32Array[] = [];
  private samples = 0;
  private queue = Promise.resolve();
  private pending = 0;
  private stopped = false;
  private failure?: unknown;
  private trackEnded = () => {
    if (this.stopped) return;
    this.onFailure(
      new Error(
        "麦克风已断开或录音权限被撤回，录音已停止，正在保存已采集音频。",
      ),
    );
    void this.stop().catch(() => undefined);
  };
  constructor(
    private onChunk: (id: string, base64: string) => Promise<void>,
    private onFailure: (error: unknown) => void,
    private chunkSeconds = 10,
  ) {}
  async start() {
    this.stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: true },
      video: false,
    });
    if (this.stopped) {
      this.stream.getTracks().forEach((t) => t.stop());
      return;
    }
    this.stream
      .getTracks()
      .forEach((track) => track.addEventListener("ended", this.trackEnded));
    try {
      this.context = new AudioContext();
      await this.context.resume();
      if (this.stopped) return;
      this.source = this.context.createMediaStreamSource(this.stream);
      this.processor = this.context.createScriptProcessor(4096, 1, 1);
      this.processor.onaudioprocess = (event) => {
        if (this.stopped) return;
        const data = event.inputBuffer.getChannelData(0).slice();
        this.chunks.push(data);
        this.samples += data.length;
        const size = Math.round(this.context!.sampleRate * this.chunkSeconds);
        while (!this.stopped && this.samples >= size) this.flush(size);
      };
      this.source.connect(this.processor);
      this.processor.connect(this.context.destination);
    } catch (error) {
      await this.stop();
      throw error;
    }
  }
  private flush(limit = this.samples) {
    if (!this.samples || !this.context) return;
    const count = Math.min(limit, this.samples);
    const combined = new Float32Array(this.samples);
    let offset = 0;
    this.chunks.forEach((c) => {
      combined.set(c, offset);
      offset += c.length;
    });
    const samples = combined.subarray(0, count);
    this.chunks = count < combined.length ? [combined.slice(count)] : [];
    this.samples -= count;
    const base64 = bytesBase64(pcmWav(samples, this.context.sampleRate)),
      id = crypto.randomUUID();
    this.pending++;
    this.queue = this.queue.then(async () => {
      try {
        await this.onChunk(id, base64);
      } catch (error) {
        this.failure = error;
        this.onFailure(error);
        void this.stop().catch(() => undefined);
      } finally {
        this.pending--;
      }
    });
    if (this.pending >= 6) {
      this.onFailure(
        new Error(
          "转录处理较慢，已暂停录音。已采集音频会继续保存，请稍后继续。",
        ),
      );
      void this.stop().catch(() => undefined);
    }
  }
  async stop() {
    if (!this.stopped) {
      this.stopped = true;
      this.flush();
      this.processor?.disconnect();
      this.source?.disconnect();
      this.stream?.getTracks().forEach((t) => {
        t.removeEventListener("ended", this.trackEnded);
        t.stop();
      });
      await this.context?.close();
    }
    await this.queue;
    if (this.failure) throw this.failure;
  }
}
