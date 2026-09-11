import type { ChatImage } from "./types";

export const MAX_CHAT_IMAGES = 4;
export const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
export const MAX_IMAGES_BYTES = 12 * 1024 * 1024;
const acceptedTypes = new Set([
  "image/png",
  "image/jpeg",
  "image/webp",
  "image/gif",
]);

export function imageBytes(image: ChatImage) {
  const base64 = image.dataUrl.split(",")[1] || "";
  return (
    (base64.length * 3) / 4 -
    (base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0)
  );
}

export async function readChatImages(
  files: File[],
  existing: ChatImage[],
): Promise<ChatImage[]> {
  if (files.length + existing.length > MAX_CHAT_IMAGES)
    throw new Error("每条消息最多添加 4 张图片，请移除部分图片后重试");
  let total = existing.reduce((sum, image) => sum + imageBytes(image), 0);
  for (const file of files) {
    if (!acceptedTypes.has(file.type))
      throw new Error("请选择 PNG、JPEG、WebP 或 GIF 图片");
    if (!file.size || file.size > MAX_IMAGE_BYTES)
      throw new Error("单张图片需大于 0 字节且不超过 5 MB");
    total += file.size;
  }
  if (total > MAX_IMAGES_BYTES)
    throw new Error("图片总大小不能超过 12 MB，请压缩或移除部分图片");
  const added = await Promise.all(
    files.map(
      (file) =>
        new Promise<ChatImage>((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () =>
            resolve({
              name: Array.from(file.name).slice(0, 128).join(""),
              dataUrl: String(reader.result),
            });
          reader.onerror = () => reject(new Error("图片读取失败，请重新选择"));
          reader.readAsDataURL(file);
        }),
    ),
  );
  return [...existing, ...added];
}
