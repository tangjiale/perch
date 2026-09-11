import { useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Paperclip, Save, Send, X } from "lucide-react";
import { command } from "../lib/api";
import {
  fromStoredDraft,
  toStoredDraft,
  type MailAccount,
  type MailDraft,
  type StoredMailDraft,
} from "../lib/mail";
import Modal from "./Modal";
import GlassSelect from "./GlassSelect";

export default function MailComposer({
  accounts,
  initial,
  onClose,
  onSaved,
}: {
  accounts: MailAccount[];
  initial: MailDraft;
  onClose: () => void;
  onSaved: (message?: string) => Promise<unknown>;
}) {
  const [draft, setDraft] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [savedMessage, setSavedMessage] = useState("");
  const [sent, setSent] = useState(false);
  const dirty = useRef(false);
  const flight = useRef(false);
  function update(value: Partial<MailDraft>) {
    dirty.current = true;
    setDraft({ ...draft, ...value });
    setSavedMessage("");
  }
  function close() {
    if (!dirty.current || confirm("正文有未保存的修改，确定放弃并关闭？"))
      onClose();
  }
  async function submit(send: boolean) {
    if (flight.current) return;
    flight.current = true;
    setBusy(true);
    setError("");
    try {
      if (sent) {
        await onSaved();
        onClose();
        return;
      }
      const saved = fromStoredDraft(
        await command<StoredMailDraft>("mail_draft_save", {
          draft: toStoredDraft(draft),
        }),
      );
      setDraft(saved);
      dirty.current = false;
      if (send) {
        const result = await command<{ message: string }>("mail_send", {
          draftId: saved.id,
        });
        setSent(true);
        setSavedMessage(result.message || "邮件已发送");
        await onSaved(result.message || "邮件已发送");
        onClose();
      } else {
        setSavedMessage("草稿已保存到本机");
        await onSaved();
      }
    } catch (e) {
      setError((e as Error).message);
    } finally {
      flight.current = false;
      setBusy(false);
    }
  }
  return (
    <Modal
      title={sent ? "邮件已发送" : "写邮件"}
      wide
      busy={busy}
      onClose={close}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void submit(true);
        }}
      >
        <fieldset
          disabled={busy || sent}
          className="form-grid provider-form-fields mail-compose-fields"
        >
          <label>
            发件邮箱
            <GlassSelect
              disabled={busy || sent}
              value={draft.accountId}
              options={accounts
                .filter((a) => a.enabled)
                .map((a) => ({ value: a.id, label: `${a.name} · ${a.email}` }))}
              onValueChange={(accountId) => update({ accountId })}
            />
          </label>
          <label>
            收件人
            <input
              required
              value={draft.to}
              placeholder="多个邮箱使用英文逗号分隔"
              onChange={(e) => update({ to: e.target.value })}
            />
          </label>
          <div className="two-fields">
            <label>
              抄送
              <input
                value={draft.cc}
                onChange={(e) => update({ cc: e.target.value })}
              />
            </label>
            <label>
              密送
              <input
                value={draft.bcc}
                onChange={(e) => update({ bcc: e.target.value })}
              />
            </label>
          </div>
          <label>
            主题
            <input
              value={draft.subject}
              maxLength={998}
              onChange={(e) => update({ subject: e.target.value })}
            />
          </label>
          <label className="mail-compose-body">
            正文
            <textarea
              value={draft.text}
              rows={10}
              placeholder="写下你想说的…"
              onChange={(e) => update({ text: e.target.value })}
            />
          </label>
          <div className="mail-compose-attachments">
            {draft.attachmentPaths.map((path) => (
              <span key={path}>
                <Paperclip size={13} />
                <span>{path.split(/[\\/]/).pop()}</span>
                <button
                  type="button"
                  className="icon-button"
                  aria-label={`移除附件 ${path.split(/[\\/]/).pop()}`}
                  onClick={() =>
                    update({
                      attachmentPaths: draft.attachmentPaths.filter(
                        (p) => p !== path,
                      ),
                    })
                  }
                >
                  <X size={13} />
                </button>
              </span>
            ))}
          </div>
          <button
            type="button"
            className="mail-add-attachment"
            onClick={async () => {
              try {
                const paths = await open({
                  multiple: true,
                  title: "选择邮件附件",
                });
                if (paths)
                  update({
                    attachmentPaths: [
                      ...new Set([
                        ...draft.attachmentPaths,
                        ...(Array.isArray(paths) ? paths : [paths]),
                      ]),
                    ],
                  });
              } catch (e) {
                setError((e as Error).message);
              }
            }}
          >
            <Paperclip size={15} />
            添加附件
          </button>
        </fieldset>
        {error && (
          <p className="error" role="alert">
            {sent ? "发送成功，但列表刷新失败：" : ""}
            {error}
          </p>
        )}
        {savedMessage && (
          <p className="mail-feedback" role="status">
            {savedMessage}
          </p>
        )}
        <footer className="mail-compose-footer">
          <span className="muted">草稿保存在本机，点击发送后才会发出。</span>
          <button
            type="button"
            disabled={busy || sent}
            onClick={() => void submit(false)}
          >
            <Save size={15} />
            保存草稿
          </button>
          <button
            className="primary"
            type="submit"
            disabled={busy || !draft.accountId || (!sent && !draft.to.trim())}
          >
            <Send size={15} />
            {busy ? "正在处理…" : sent ? "重试刷新" : "发送"}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
