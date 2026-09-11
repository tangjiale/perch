import { describe, expect, it } from "vitest";
import { readChatImages, imageBytes, MAX_IMAGE_BYTES } from "./chat-images";

describe("聊天图片校验", () => {
  it("准确计算带填充的图片大小", () => {
    expect(
      imageBytes({ name: "a", dataUrl: "data:image/png;base64,YQ==" }),
    ).toBe(1);
    expect(
      imageBytes({ name: "a", dataUrl: "data:image/png;base64,YWI=" }),
    ).toBe(2);
  });
  it("超数量、错误格式与空文件在读取前被拒绝", async () => {
    const existing = Array.from({ length: 4 }, () => ({
      name: "a",
      dataUrl: "data:image/png;base64,YQ==",
    }));
    await expect(
      readChatImages(
        [new File(["a"], "a.png", { type: "image/png" })],
        existing,
      ),
    ).rejects.toThrow("最多添加 4");
    await expect(
      readChatImages(
        [new File(["<svg/>"], "a.svg", { type: "image/svg+xml" })],
        [],
      ),
    ).rejects.toThrow("请选择");
    await expect(
      readChatImages([new File([], "a.png", { type: "image/png" })], []),
    ).rejects.toThrow("大于 0");
  });
  it("限制单张与累计大小", async () => {
    const oversized = new File([new Uint8Array(MAX_IMAGE_BYTES + 1)], "a.png", {
      type: "image/png",
    });
    await expect(readChatImages([oversized], [])).rejects.toThrow("5 MB");
    const large = new File([new Uint8Array(MAX_IMAGE_BYTES)], "a.png", {
      type: "image/png",
    });
    await expect(readChatImages([large, large, large], [])).rejects.toThrow(
      "12 MB",
    );
  });
});
