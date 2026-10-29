import { afterEach, describe, expect, it, vi } from "vitest";
import { MeetingRecorder } from "./meeting-recorder";
function mockMicrophone(sampleRate = 16000) {
  const trackStop = vi.fn(),
    disconnect = vi.fn();
  const processor = {
    onaudioprocess: undefined as undefined | ((event: unknown) => void),
    connect: vi.fn(),
    disconnect,
  };
  const close = vi.fn(async () => {});
  const track = new EventTarget();
  Object.assign(track, { stop: trackStop });
  vi.stubGlobal("navigator", {
    mediaDevices: {
      getUserMedia: vi.fn(async () => ({
        getTracks: () => [track],
      })),
    },
  });
  vi.stubGlobal(
    "AudioContext",
    class {
      sampleRate = sampleRate;
      destination = {};
      resume = async () => {};
      close = close;
      createMediaStreamSource = () => ({ connect: vi.fn(), disconnect });
      createScriptProcessor = () => processor;
    },
  );
  return {
    emit: (size: number) =>
      processor.onaudioprocess?.({
        inputBuffer: { getChannelData: () => new Float32Array(size).fill(0.1) },
      }),
    trackStop,
    end: () => track.dispatchEvent(new Event("ended")),
    close,
  };
}
afterEach(() => vi.unstubAllGlobals());
describe("会议录音生命周期", () => {
  it("停止时刷新尾部并等待存储完成，立即释放麦克风", async () => {
    const mic = mockMicrophone();
    let finish!: () => void;
    const saved = vi.fn(
      (_id: string, _base64: string) =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    const recorder = new MeetingRecorder(saved, vi.fn());
    await recorder.start();
    mic.emit(160);
    let ended = false;
    const stopped = recorder.stop().then(() => {
      ended = true;
    });
    await Promise.resolve();
    expect(mic.trackStop).toHaveBeenCalledOnce();
    expect(mic.close).toHaveBeenCalledOnce();
    expect(saved).toHaveBeenCalledOnce();
    expect(ended).toBe(false);
    const bytes = Uint8Array.from(atob(saved.mock.calls[0][1] as string), (c) =>
      c.charCodeAt(0),
    );
    expect(new DataView(bytes.buffer).getUint32(40, true)).toBe(320);
    finish();
    await stopped;
    expect(ended).toBe(true);
  });
  it("保存失败后停止采集，但仍处理排队的尾片且不自动重发未知结果", async () => {
    const mic = mockMicrophone(),
      failure = new Error("存储结果未知"),
      fail = vi.fn();
    let count = 0;
    const saved = vi.fn(async () => {
      count++;
      if (count === 1) throw failure;
    });
    const recorder = new MeetingRecorder(saved, fail);
    await recorder.start();
    mic.emit(160000);
    mic.emit(160);
    await expect(recorder.stop()).rejects.toThrow("存储结果未知");
    expect(saved).toHaveBeenCalledTimes(2);
    expect(fail).toHaveBeenCalledWith(failure);
    expect(mic.trackStop).toHaveBeenCalledOnce();
  });
  it("慢存储达到队列上限主动释放设备并排空已有片段", async () => {
    const mic = mockMicrophone(),
      fail = vi.fn(),
      saved = vi.fn(async () => {}),
      recorder = new MeetingRecorder(saved, fail);
    await recorder.start();
    for (let i = 0; i < 9; i++) mic.emit(160000);
    await recorder.stop();
    expect(saved).toHaveBeenCalledTimes(6);
    expect(fail).toHaveBeenCalledOnce();
    expect(mic.trackStop).toHaveBeenCalledOnce();
  });
});

it("设备断开时停止录音并保存不足十秒的尾部，正常stop移除监听", async () => {
  const mic = mockMicrophone(),
    saved = vi.fn(async (_id: string, _base64: string) => {}),
    failure = vi.fn();
  const recorder = new MeetingRecorder(saved, failure);
  await recorder.start();
  mic.emit(1600);
  mic.end();
  await recorder.stop();
  expect(saved).toHaveBeenCalledOnce();
  expect(failure).toHaveBeenCalledOnce();
  expect(String(failure.mock.calls[0][0])).toContain("麦克风已断开");
  expect(mic.trackStop).toHaveBeenCalledOnce();
  mic.end();
  expect(failure).toHaveBeenCalledOnce();
});

it("实时模式在停止前逐秒保存，保留音频事件越过边界的余量", async () => {
  const mic = mockMicrophone(),
    saved = vi.fn(async (_id: string, _base64: string) => {});
  const recorder = new MeetingRecorder(saved, vi.fn(), 1);
  await recorder.start();
  mic.emit(20000);
  await Promise.resolve();
  expect(saved).toHaveBeenCalledOnce();
  const size = (base64: string) => {
    const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
    return new DataView(bytes.buffer).getUint32(40, true);
  };
  expect(size(saved.mock.calls[0][1])).toBe(32000);
  mic.emit(12000);
  await Promise.resolve();
  await recorder.stop();
  expect(saved).toHaveBeenCalledTimes(2);
  expect(saved.mock.calls.map((call) => size(call[1]))).toEqual([32000, 32000]);
});

it("默认分片模式不足十秒不会提前保存，停止时保存尾部", async () => {
  const mic = mockMicrophone(),
    saved = vi.fn(async () => {});
  const recorder = new MeetingRecorder(saved, vi.fn());
  await recorder.start();
  mic.emit(16000);
  await Promise.resolve();
  expect(saved).not.toHaveBeenCalled();
  await recorder.stop();
  expect(saved).toHaveBeenCalledOnce();
});

it("44.1kHz 输入的实时分片保持精确一秒，音频回调余量不重复填充", async () => {
  const mic = mockMicrophone(44100),
    saved = vi.fn(async (_id: string, _base64: string) => {});
  const recorder = new MeetingRecorder(saved, vi.fn(), 1);
  await recorder.start();
  // 实际音频回调不会恰好落在秒边界。
  for (let remaining = 88200; remaining > 0;) {
    const size = Math.min(4096, remaining);
    mic.emit(size);
    remaining -= size;
    await Promise.resolve();
  }
  await recorder.stop();
  expect(saved).toHaveBeenCalledTimes(2);
  for (const [, base64] of saved.mock.calls) {
    const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
    expect(new DataView(bytes.buffer).getUint32(40, true)).toBe(32000);
  }
});
