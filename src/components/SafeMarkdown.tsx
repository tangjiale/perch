import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";
import { native } from "../lib/api";

function safeExternalUrl(href?: string): string | null {
  if (!href) return null;
  try {
    const parsed = new URL(href);
    return parsed.protocol === "http:" || parsed.protocol === "https:"
      ? parsed.toString()
      : null;
  } catch {
    return null;
  }
}

export default function SafeMarkdown({
  children,
  onError,
}: {
  children: string;
  onError?: (message: string) => void;
}) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        img: () => null,
        a: ({ href, children: content }) => {
          const target = safeExternalUrl(href);
          if (!target) return <span>{content}</span>;
          return (
            <a
              href={target}
              target="_blank"
              rel="noopener noreferrer"
              onClick={(event) => {
                if (!native) return;
                event.preventDefault();
                void openUrl(target).catch((error) =>
                  onError?.(
                    `打开链接失败：${error instanceof Error ? error.message : String(error)}`,
                  ),
                );
              }}
            >
              {content}
            </a>
          );
        },
      }}
    >
      {children}
    </ReactMarkdown>
  );
}
