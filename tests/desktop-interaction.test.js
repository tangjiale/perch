import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

const script = readFileSync(new URL("../src-tauri/src/desktop-interaction.js", import.meta.url), "utf8");

function contextMenu({ image = true, marked = true, complete = true, naturalWidth = 100,
  src = "data:image/png;base64,iVBORw0KGgo=", currentSrc = "", childFrame = false } = {}) {
  class HTMLImageElement {
    constructor() { Object.assign(this, { complete, naturalWidth, src, currentSrc }); }
    hasAttribute(name) { return marked && name === "data-copyable-image"; }
  }
  let listener;
  const window = {
    addEventListener: vi.fn((type, callback) => {
      if (type === "contextmenu") listener = callback;
    }),
  };
  window.top = childFrame ? {} : window;
  runInNewContext(script, { window, HTMLImageElement });
  expect(window.addEventListener).toHaveBeenCalledWith("contextmenu", expect.any(Function), true);
  const event = { target: image ? new HTMLImageElement() : {}, preventDefault: vi.fn() };
  listener(event);
  return event.preventDefault;
}

describe("桌面默认右键菜单屏蔽", () => {
  it.each(["png", "jpeg", "gif", "webp"])("主页面已加载的标记 %s 图片也屏蔽系统菜单", (format) => {
    expect(contextMenu({ src: `data:image/${format};base64,AAAA` })).toHaveBeenCalledOnce();
  });

  it.each([
    ["普通区域", { image: false }],
    ["未标记图片", { marked: false }],
    ["远端图片地址", { src: "https://example.com/image.png" }],
    ["SVG 图片", { src: "data:image/svg+xml;base64,AAAA" }],
    ["尚未加载的图片", { complete: false }],
    ["加载损坏的图片", { naturalWidth: 0 }],
    ["邮件等子 frame 图片", { childFrame: true }],
    ["实际来源为远端的图片", { currentSrc: "https://example.com/image.png" }],
  ])("%s 仍禁用默认菜单", (_label, options) => {
    expect(contextMenu(options)).toHaveBeenCalledOnce();
  });

  it("优先使用浏览器实际加载的图片来源", () => {
    expect(contextMenu({ src: "https://example.com/image.png", currentSrc: "data:image/png;base64,AAAA" }))
      .toHaveBeenCalledOnce();
  });
});
