import { useEffect, useRef, useState } from "react";
import { ImageIcon, RefreshCw, Upload } from "lucide-react";
import { command, native } from "../lib/api";

function websiteUrl(value: string): string | null {
  try {
    const url = new URL(value);
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.username ||
      url.password
    )
      return null;
    url.hash = "";
    url.search = "";
    return url.href;
  } catch {
    return null;
  }
}

function verifyImage(source: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () =>
      image.naturalWidth && image.naturalHeight
        ? resolve()
        : reject(Error("图片内容无效"));
    image.onerror = () => reject(Error("图片无法读取，请选择其他图片"));
    image.src = source;
  });
}

export default function AppLogoPicker({
  url,
  value,
  onChange,
}: {
  url: string;
  value?: string;
  onChange: (logo: string) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const generation = useRef(0);
  const source = useRef(value ? "existing" : "auto");
  const lastAutoUrl = useRef<string | null>(null);
  const callback = useRef(onChange);
  const [status, setStatus] = useState(value ? "已设置图片" : "未选择文件");
  const [busy, setBusy] = useState(false);
  const [retry, setRetry] = useState(0);
  const address = websiteUrl(url);
  useEffect(() => {
    callback.current = onChange;
  }, [onChange]);
  useEffect(
    () => () => {
      generation.current++;
    },
    [],
  );

  useEffect(() => {
    const request = ++generation.current;
    setBusy(false);
    if (
      source.current === "auto" &&
      lastAutoUrl.current &&
      lastAutoUrl.current !== address
    ) {
      callback.current("");
      lastAutoUrl.current = null;
      setStatus("未选择文件");
    }
    if (source.current !== "auto") return;
    if (!address) {
      setStatus("未选择文件");
      return;
    }
    if (!native) {
      setStatus("可在桌面应用中自动获取");
      return;
    }
    const timer = window.setTimeout(async () => {
      if (request !== generation.current || source.current !== "auto") return;
      setBusy(true);
      setStatus("正在获取网站图标…");
      try {
        const logo = await command<string>("website_icon_fetch", {
          url: address,
        });
        await verifyImage(logo);
        if (request !== generation.current) return;
        callback.current(logo);
        lastAutoUrl.current = address;
        setStatus("已获取网站图标");
      } catch {
        if (request === generation.current)
          setStatus("未获取到图标，可手动选择图片");
      } finally {
        if (request === generation.current) setBusy(false);
      }
    }, 800);
    return () => {
      clearTimeout(timer);
      generation.current++;
    };
  }, [address, retry]);

  async function upload(file: File) {
    if (file.size > 2 * 1024 * 1024) {
      setStatus("图片不能超过 2 MB");
      return;
    }
    if (!["image/png", "image/jpeg", "image/webp"].includes(file.type)) {
      setStatus("请选择 PNG、JPEG 或 WebP 图片");
      return;
    }
    const request = ++generation.current;
    source.current = "manual";
    setBusy(true);
    setStatus("正在读取图片…");
    try {
      const logo = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result));
        reader.onerror = () => reject(Error("读取失败"));
        reader.readAsDataURL(file);
      });
      await verifyImage(logo);
      if (request !== generation.current) return;
      callback.current(logo);
      setStatus(file.name);
    } catch {
      if (request === generation.current) setStatus("图片读取失败，请重新选择");
    } finally {
      if (request === generation.current) setBusy(false);
    }
  }
  return (
    <div className="logo-file-field">
      <span id="app-logo-label">Logo</span>
      <div className="logo-file-control">
        <div className="logo-picker-preview">
          {value ? (
            <img src={value} alt="应用图标预览" />
          ) : (
            <ImageIcon size={20} aria-hidden="true" />
          )}
        </div>
        <button type="button" onClick={() => input.current?.click()}>
          <Upload size={15} />
          选择图片
        </button>
        <button
          type="button"
          className="icon-button"
          title="重新获取网站图标"
          disabled={!address || !native || busy}
          onClick={() => {
            source.current = "auto";
            setRetry((count) => count + 1);
          }}
        >
          <RefreshCw size={16} />
        </button>
      </div>
      <span className="logo-file-status" role="status" title={status}>
        {status}
      </span>
      <input
        ref={input}
        hidden
        type="file"
        aria-labelledby="app-logo-label"
        accept="image/png,image/jpeg,image/webp"
        onChange={(event) => {
          const file = event.target.files?.[0];
          event.target.value = "";
          if (file) void upload(file);
        }}
      />
    </div>
  );
}
