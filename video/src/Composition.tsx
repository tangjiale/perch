import { AbsoluteFill, Sequence, staticFile, useCurrentFrame } from 'remotion';
import { Audio, Video } from '@remotion/media';
import captures from '../capture-manifest.json';
import voices from '../voice-manifest.json';
import { cameraAt, subtitlePages, font } from './design';

export interface CaptureStep {
  id: string;
  start: number;
  end: number;
  zoom: number;
  x: number;
  y: number;
  delay?: number;
}
export interface CaptureClip {
  id: string;
  title: string;
  file: string;
  duration: number;
  steps: CaptureStep[];
}
export const FPS = 30;
export const captureClips = captures as CaptureClip[];
export const clipFrames = (clip: CaptureClip) => Math.max(1, Math.round(clip.duration * FPS));
export const durationInFrames = Math.max(1, captureClips.reduce((total, clip) => total + clipFrames(clip), 0));

function Caption({ text, duration, top }: { text: string; duration: number; top: boolean }) {
  const frame = useCurrentFrame();
  const pages = subtitlePages(text);
  const page = pages[Math.min(pages.length - 1, Math.floor(frame / Math.max(1, duration) * pages.length))];
  return <div style={{ position: 'absolute', ...(top ? { top: 75 } : { bottom: 32 }), left: '12%', width: '76%', display: 'flex', justifyContent: 'center', pointerEvents: 'none' }}>
    <div style={{ color: 'white', fontFamily: font, fontSize: 31, fontWeight: 500, lineHeight: 1.55, textAlign: 'center', background: 'rgba(12, 19, 24, .77)', padding: '11px 25px', borderRadius: 9, whiteSpace: 'pre-line', textShadow: '0 1px 3px #0008' }}>{page}</div>
  </div>;
}

function Recording({ clip }: { clip: CaptureClip }) {
  const frame = useCurrentFrame();
  const camera = cameraAt(frame / FPS, clip.steps);
  const duration = clipFrames(clip);
  return <AbsoluteFill style={{ overflow: 'hidden', background: '#f4f7fa' }}>
    {/* 录屏包含实际鼠标；相机统一变换整帧，鼠标与界面保持同一坐标系。 */}
    <AbsoluteFill style={{ transformOrigin: '0 0', transform: `translate(${camera.tx * 100}%, ${camera.ty * 100}%) scale(${camera.zoom})` }}>
      <Video src={staticFile(clip.file)} muted objectFit="cover" style={{ width: '100%', height: '100%' }} />
    </AbsoluteFill>
    {clip.steps.map((step, index) => {
      const voice = voices.find(item => item.id === step.id);
      if (!voice) return null;
      const from = Math.round((step.start + .1) * FPS);
      const length = Math.min(Math.ceil(voice.duration * FPS), duration - from);
      if (length <= 0) return null;
      return <Sequence key={`${step.id}-${index}`} from={from} durationInFrames={length} name={voice.text}>
        <Audio src={staticFile(voice.file)} />
        <Caption text={voice.text} duration={length} top={clip.id === "tasks" || clip.id === "chat"} />
      </Sequence>;
    })}
  </AbsoluteFill>;
}

export const PerchIntro = () => {
  let cursor = 0;
  return <AbsoluteFill style={{ background: '#f4f7fa' }}>
    {captureClips.map(clip => {
      const from = cursor;
      const duration = clipFrames(clip);
      cursor += duration;
      return <Sequence key={clip.id} from={from} durationInFrames={duration} name={clip.title}>
        <Recording clip={clip} />
      </Sequence>;
    })}
  </AbsoluteFill>;
};
