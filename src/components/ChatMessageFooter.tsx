import { Check, Clock3, Copy, Database } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Message } from "../lib/types";
import { messageMetadata } from "../lib/message-metadata";

export default function ChatMessageFooter({
  message,
  notify,
}: {
  message: Message;
  notify: (text: string) => void;
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const metadata = messageMetadata(message);
  return (
    <footer className="chat-message-footer">
      <button
        type="button"
        className="icon-button"
        title={copied ? "已复制" : "复制回复"}
        aria-label={copied ? "已复制回复" : "复制回复"}
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(message.content);
            setCopied(true);
            clearTimeout(timer.current);
            timer.current = setTimeout(() => setCopied(false), 1800);
          } catch {
            notify("复制失败，请检查剪贴板权限");
          }
        }}
      >
        {copied ? <Check size={15} /> : <Copy size={15} />}
      </button>
      <span title={metadata.usageDetail}>
        <Database size={14} aria-hidden="true" />
        {metadata.usage}
      </span>
      <span>
        <Clock3 size={14} aria-hidden="true" />
        {metadata.duration}
      </span>
      {metadata.time && (
        <time dateTime={metadata.isoTime} title={metadata.fullTime}>
          {metadata.time}
        </time>
      )}
    </footer>
  );
}
