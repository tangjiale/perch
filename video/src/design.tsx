export const font = '"PingFang SC", "Noto Sans CJK SC", "Microsoft YaHei", sans-serif';

interface CameraStep {
  start: number;
  end: number;
  zoom: number;
  x: number;
  y: number;
  delay?: number;
}
const bounded = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));
function cameraTarget(step?: CameraStep) {
  const zoom = bounded(step?.zoom ?? 1, 1, 3);
  // tx/ty 是整个画布的归一平移量，限制在放大后画布覆盖视窗的范围。
  return { zoom, tx: bounded(.5 - bounded(step?.x ?? .5, 0, 1) * zoom, 1 - zoom, 0), ty: bounded(.5 - bounded(step?.y ?? .5, 0, 1) * zoom, 1 - zoom, 0) };
}
export function cameraAt(seconds: number, steps: CameraStep[]) {
  let camera = cameraTarget();
  for (const step of [...steps].sort((a, b) => a.start - b.start)) {
    const start = step.start + Math.max(0, step.delay ?? 1.5);
    if (seconds < start) break;
    const next = cameraTarget(step);
    const linear = bounded((seconds - start) / .65, 0, 1);
    const amount = linear * linear * (3 - 2 * linear);
    camera = { zoom: camera.zoom + (next.zoom - camera.zoom) * amount, tx: camera.tx + (next.tx - camera.tx) * amount, ty: camera.ty + (next.ty - camera.ty) * amount };
  }
  return camera;
}

/** 长句按中文标点优先换行，每行最多 28 字，每页最多两行。 */
export function subtitlePages(text: string): string[] {
  const chars = Array.from(text.trim());
  const lines: string[] = [];
  while (chars.length) {
    let count = Math.min(28, chars.length);
    if (count < chars.length) {
      for (let index = count - 1; index >= 12; index--) {
        if ('，。；！？、：'.includes(chars[index])) { count = index + 1; break; }
      }
    }
    lines.push(chars.splice(0, count).join(''));
  }
  const pages: string[] = [];
  for (let index = 0; index < lines.length; index += 2) pages.push(lines.slice(index, index + 2).join('\n'));
  return pages.length ? pages : [''];
}
