// node tests/image-viewer-browser.js http://127.0.0.1:5173
// 使用真实浏览器鼠标和键盘，仅访问隔离 fixture，不读写真实禅道。
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";

const origin = process.argv[2] || "http://127.0.0.1:5173";
const run = (...args) => execFileSync("npx", ["--yes", "agent-browser", "--session", "image-viewer-test", ...args], { encoding: "utf8", timeout: 60000 });
const evaluate = (code) => JSON.parse(JSON.parse(run("eval", `JSON.stringify(${code})`).trim()));
const wait = () => run("wait", "180");
const state = () => evaluate(`(() => {
  const dialog = document.querySelector('dialog.image-viewer');
  const stage = dialog.querySelector('.image-viewer-stage');
  const image = dialog.querySelector('.image-viewer-image');
  const rect = image.getBoundingClientRect();
  return { scale: Number(stage.dataset.scale), width: rect.width, x: rect.x, y: rect.y,
    centerX: Math.round(rect.x + rect.width / 2), centerY: Math.round(rect.y + rect.height / 2),
    fitScale: Math.min(1, stage.clientWidth / image.naturalWidth, stage.clientHeight / image.naturalHeight),
    dialogWidth: dialog.getBoundingClientRect().width, viewportWidth: innerWidth };
})()`);
// CLI 的独立 mouse 命令会将滚轮坐标重置为 (0,0)。通过同一会话的
// CDP 发送带坐标的原生输入，保持 isTrusted，避免用合成 DOM 事件替代。
run("open", `${origin}/tests/bugs.html`);
run("set", "viewport", "1440", "900");
const socket = new WebSocket(run("get", "cdp-url").trim());
const pending = new Map();
try {
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("CDP WebSocket 建连超过 10 秒")), 10000);
    const complete = callback => value => {
      clearTimeout(timeout);
      socket.removeEventListener("open", opened);
      socket.removeEventListener("error", failed);
      callback(value);
    };
    const opened = complete(resolve);
    const failed = complete(() => reject(new Error("CDP WebSocket 建连失败")));
    socket.addEventListener("open", opened, { once: true });
    socket.addEventListener("error", failed, { once: true });
  });
  let nextId = 0;
  socket.addEventListener("message", event => {
    const value = JSON.parse(event.data);
    if (!value.id) return;
    const callback = pending.get(value.id);
    if (!callback) return;
    clearTimeout(callback.timeout);
    pending.delete(value.id);
    if (value.error) callback.reject(new Error(value.error.message));
    else callback.resolve(value.result);
  });
  const cdp = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
    const id = ++nextId;
    const timeout = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`CDP ${method} 响应超过 10 秒`));
    }, 10000);
    pending.set(id, { resolve, reject, timeout });
    try {
      socket.send(JSON.stringify({ id, method, params, sessionId }));
    } catch (error) {
      clearTimeout(timeout);
      pending.delete(id);
      reject(error);
    }
  });
  const { targetInfos } = await cdp("Target.getTargets");
  const target = targetInfos.find(item => item.url === `${origin}/tests/bugs.html`);
  const { sessionId } = await cdp("Target.attachToTarget", { targetId: target.targetId, flatten: true });
  const mouse = params => cdp("Input.dispatchMouseEvent", params, sessionId);
  run("click", ".bugs-title");
  wait();
  run("click", '[aria-label="放大查看复现步骤图片"]');
  wait();
  const fit = state();
  assert.ok(fit.dialogWidth > 650 && fit.dialogWidth > fit.viewportWidth * 0.85, "预览画布应大于旧详情弹窗且占窗口至少 85%");
  run("click", ".image-viewer-image");
  wait();
  const enlarged = state();
  assert.ok(enlarged.width > fit.width * 1.15, `点击图片必须明显放大：${JSON.stringify({ fit, enlarged })}`);
  await mouse({ type: "mouseWheel", x: 720, y: 450, deltaY: -240, deltaX: 0 });
  wait();
  assert.ok(state().scale > enlarged.scale, "滚轮向上放大图片");
  run("find", "role", "button", "click", "--name", "原始尺寸", "--exact");
  wait();
  assert.equal(state().scale, 1, "原始尺寸等于自然像素比例 1");
  run("click", '[aria-label="放大图片"]');
  wait();
  const buttonZoom = state().scale;
  assert.ok(buttonZoom > 1, "放大按钮可放大");
  run("click", '[aria-label="缩小图片"]');
  wait();
  assert.ok(state().scale < buttonZoom, "缩小按钮降低倍率");
  run("click", '[aria-label="放大图片"]');
  wait();
  const beforeDrag = state();
  await mouse({ type: "mouseMoved", x: 700, y: 440 });
  await mouse({ type: "mousePressed", x: 700, y: 440, button: "left", buttons: 1, clickCount: 1 });
  await mouse({ type: "mouseMoved", x: 800, y: 490, button: "left", buttons: 1 });
  await mouse({ type: "mouseReleased", x: 800, y: 490, button: "left", buttons: 0, clickCount: 1 });
  wait();
  const dragged = state();
  assert.equal(dragged.scale, beforeDrag.scale, "拖动放开不能误触点击还原");
  assert.ok(Math.abs(dragged.x - beforeDrag.x) > 30 || Math.abs(dragged.y - beforeDrag.y) > 30, "放大后拖动应平移图片");
  run("find", "role", "button", "click", "--name", "适应窗口", "--exact");
  wait();
  const fitted = state();
  assert.ok(Math.abs(fitted.scale - fitted.fitScale) < 0.01, "适应窗口恢复当前画布比例");
  const blank = evaluate(`(() => { const s = document.querySelector(".image-viewer-stage").getBoundingClientRect(); const i = document.querySelector(".image-viewer-image").getBoundingClientRect(); const p = [{x:s.left+3,y:s.top+3},{x:s.right-3,y:s.bottom-3}].find(p => p.x<i.left || p.x>i.right || p.y<i.top || p.y>i.bottom); if (!p) throw Error("fixture 无画布留白"); return {x:Math.round(p.x),y:Math.round(p.y)}; })()`);
  await mouse({ type: "mousePressed", x: blank.x, y: blank.y, button: "left", buttons: 1, clickCount: 1 });
  await mouse({ type: "mouseReleased", x: blank.x, y: blank.y, button: "left", buttons: 0, clickCount: 1 });
  wait();
  assert.equal(evaluate(`document.querySelectorAll('dialog[open]').length`), 2, "点击图片画布空白不关闭预览");
  run("press", "Escape");
  wait();
  assert.equal(evaluate(`document.querySelectorAll('dialog[open]').length`), 2, "Esc 不关闭图片预览或 BUG 详情");
  run("click", 'dialog.image-viewer button[title="关闭"]');
  wait();
  assert.equal(evaluate(`document.querySelectorAll('dialog[open]').length`), 1, "右上角关闭返回 BUG 详情");
  assert.equal(evaluate(`window.__bugCalls.filter(call => ['zentao_bug_save', 'zentao_bug_transition'].includes(call.name)).length`), 0, "看图操作不能修改复现步骤或提交 BUG");
  console.log(JSON.stringify({ clickZoom: true, wheelZoom: true, originalSize: true, buttons: true, dragPan: true, fit: true, escape: true, unchangedSteps: true }));
} finally {
  for (const request of pending.values()) clearTimeout(request.timeout);
  pending.clear();
  socket.close();
}
