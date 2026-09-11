import { useQuery } from "@tanstack/react-query";
import { command } from "../lib/api";
import type { StoredMailDraft } from "../lib/mail";
import Modal from "./Modal";

export default function MailDeliveryHistory({
  accountId,
  onClose,
}: {
  accountId: string;
  onClose: () => void;
}) {
  const {
    data = [],
    error,
    isLoading,
  } = useQuery({
    queryKey: ["mail", "delivery", accountId],
    queryFn: () =>
      command<(StoredMailDraft & { deliveryState: string })[]>(
        "mail_delivery_history",
        { accountId },
      ),
  });
  return (
    <Modal title="发送记录" wide onClose={onClose}>
      <p className="muted">
        已发送表示 SMTP
        服务器已接受，不代表收件人已阅读。发送中或未确认的记录不会自动重发，请先在网页邮箱核对。
      </p>
      {error && (
        <p className="error" role="alert">
          {error.message}
        </p>
      )}
      {!data.length && (
        <p className="mail-empty">
          {isLoading ? "正在读取…" : "还没有发送记录"}
        </p>
      )}
      {data.map((draft) => (
        <details className="mail-delivery-record" key={draft.id}>
          <summary>
            <strong>{draft.subject || "（无主题）"}</strong>
            <span>
              {draft.deliveryState === "sent"
                ? "已发送"
                : draft.deliveryState === "sending"
                  ? "发送中 / 待核对"
                  : "结果未确认"}
            </span>
          </summary>
          <p className="muted">
            收件人：{draft.to.join(", ")}
            {draft.cc.length ? `；抄送：${draft.cc.join(", ")}` : ""}
          </p>
          <pre>{draft.text}</pre>
          {draft.attachments.length > 0 && (
            <p className="muted">
              附件：
              {draft.attachments.map((p) => p.split(/[\\/]/).pop()).join("、")}
            </p>
          )}
        </details>
      ))}
    </Modal>
  );
}
