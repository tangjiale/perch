import { useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Mail, Plus, Settings2, ShieldCheck, Trash2 } from "lucide-react";
import { command } from "../lib/api";
import { mailApi, mailKeys, type MailAccount } from "../lib/mail";
import GlassSelect from "../components/GlassSelect";
import Modal from "../components/Modal";
import "./mail.css";

const presets = [
  { value: "custom", label: "自定义邮箱", imap: "", smtp: "" },
  {
    value: "foxmail",
    label: "QQ / Foxmail",
    imap: "imap.qq.com",
    smtp: "smtp.qq.com",
  },
  {
    value: "aliyun",
    label: "阿里企业邮箱",
    imap: "imap.qiye.aliyun.com",
    smtp: "smtp.qiye.aliyun.com",
  },
  {
    value: "163",
    label: "网易 163",
    imap: "imap.163.com",
    smtp: "smtp.163.com",
  },
  {
    value: "gmail",
    label: "Gmail",
    imap: "imap.gmail.com",
    smtp: "smtp.gmail.com",
  },
];
const securities = [
  { value: "tls", label: "SSL / TLS" },
  { value: "starttls", label: "STARTTLS" },
];
function newAccount(): MailAccount {
  return {
    id: crypto.randomUUID(),
    name: "",
    email: "",
    senderName: "",
    username: "",
    imapHost: "",
    imapPort: 993,
    imapSecurity: "tls",
    smtpHost: "",
    smtpPort: 465,
    smtpSecurity: "tls",
    smtpUsername: "",
    enabled: true,
    syncIntervalSeconds: 60,
  };
}
export default function MailSettings() {
  const client = useQueryClient();
  const { data: accounts = [], error: queryError } = useQuery({
    queryKey: mailKeys.accounts,
    queryFn: mailApi.accounts,
  });
  const [editing, setEditing] = useState<MailAccount | null>(null);
  const [imapSecret, setImapSecret] = useState("");
  const [smtpSecret, setSmtpSecret] = useState("");
  const [preset, setPreset] = useState("custom");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const inFlight = useRef(false);
  function edit(account: MailAccount) {
    setEditing(account);
    setImapSecret("");
    setSmtpSecret("");
    setPreset("custom");
    setError("");
    setMessage("");
  }
  function update(values: Partial<MailAccount>) {
    if (editing) setEditing({ ...editing, ...values });
  }
  async function save(event: React.FormEvent) {
    event.preventDefault();
    if (!editing || inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError("");
    try {
      const value = {
        ...editing,
        senderName: editing.senderName || editing.name,
        username: editing.username || editing.email,
        smtpUsername: editing.smtpUsername || editing.username || editing.email,
      };
      const saved = await command<MailAccount>("mail_account_save", {
        account: value,
        imapSecret: imapSecret || null,
        smtpSecret: smtpSecret || null,
      });
      setEditing(saved);
      setImapSecret("");
      setSmtpSecret("");
      await client.invalidateQueries({ queryKey: ["mail"] });
      setEditing(null);
      setMessage("邮箱配置已保存，后台将自动检查新邮件。");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }
  async function test(account: MailAccount) {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setMessage("");
    setError("");
    try {
      setMessage(
        await command<string>("mail_account_test", { accountId: account.id }),
      );
    } catch (e) {
      setError((e as Error).message);
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }
  return (
    <section className="mail-settings">
      <div className="section-heading">
        <div>
          <h2>邮箱设置</h2>
          <p className="muted">让工作邮件和个人邮箱，都有一个安放的位置。</p>
        </div>
        <button className="primary" onClick={() => edit(newAccount())}>
          <Plus size={16} />
          添加邮箱
        </button>
      </div>
      <p className="mail-settings-note">
        <ShieldCheck size={16} />
        使用加密连接；密码或邮箱授权码保存在系统钥匙串。
      </p>
      {(error || queryError) && !editing && (
        <p className="error" role="alert">
          {error || queryError?.message}
        </p>
      )}
      {message && (
        <p className="mail-feedback" role="status">
          {message}
        </p>
      )}
      {accounts.length ? (
        <div className="mail-account-settings-list">
          {accounts.map((account) => (
            <article className="mail-account-setting" key={account.id}>
              <span className="mail-account-avatar">
                <Mail size={20} />
              </span>
              <div className="mail-account-description">
                <h3>
                  {account.name}
                  <span className="mail-state">
                    {account.enabled ? "已启用" : "已停用"}
                  </span>
                </h3>
                <p>{account.email}</p>
                <small>
                  IMAP · {account.imapHost} · 每 {account.syncIntervalSeconds}{" "}
                  秒检查
                </small>
                {account.lastError && (
                  <p className="error">{account.lastError}</p>
                )}
              </div>
              <div className="mail-account-actions">
                <button disabled={busy} onClick={() => void test(account)}>
                  测试连接
                </button>
                <button disabled={busy} onClick={() => edit(account)}>
                  <Settings2 size={15} />
                  配置
                </button>
              </div>
            </article>
          ))}
        </div>
      ) : (
        <div className="mail-onboarding">
          <Mail size={32} />
          <h3>连接你的第一个邮箱</h3>
          <p>
            支持 QQ / Foxmail、阿里企业邮箱、网易及其他提供 IMAP / SMTP 的邮箱。
          </p>
          <button onClick={() => edit(newAccount())}>添加邮箱</button>
        </div>
      )}
      {editing && (
        <Modal
          title={editing.revision ? "邮箱配置" : "添加邮箱"}
          wide
          busy={busy}
          onClose={() => {
            setEditing(null);
            setImapSecret("");
            setSmtpSecret("");
          }}
        >
          <form onSubmit={save}>
            <fieldset
              className="form-grid provider-form-fields"
              disabled={busy}
            >
              <div className="two-fields">
                <label>
                  邮箱服务
                  <GlassSelect
                    value={preset}
                    disabled={busy}
                    onValueChange={(value) => {
                      setPreset(value);
                      const p = presets.find((p) => p.value === value)!;
                      if (value !== "custom")
                        update({
                          imapHost: p.imap,
                          smtpHost: p.smtp,
                          imapPort: 993,
                          smtpPort: 465,
                          imapSecurity: "tls",
                          smtpSecurity: "tls",
                        });
                    }}
                    options={presets}
                  />
                </label>
                <label>
                  显示名称
                  <input
                    required
                    maxLength={128}
                    value={editing.name}
                    placeholder="例如：工作邮箱"
                    onChange={(e) => update({ name: e.target.value })}
                  />
                </label>
              </div>
              <div className="two-fields">
                <label>
                  邮箱地址
                  <input
                    required
                    type="email"
                    value={editing.email}
                    onChange={(e) => update({ email: e.target.value })}
                  />
                </label>
                <label>
                  发件人名称
                  <input
                    value={editing.senderName}
                    placeholder="收件人看到的名字"
                    onChange={(e) => update({ senderName: e.target.value })}
                  />
                </label>
              </div>
              <h3 className="mail-form-divider">收信服务器 · IMAP</h3>
              <div className="mail-server-fields">
                <label>
                  服务器
                  <input
                    required
                    value={editing.imapHost}
                    placeholder="imap.example.com"
                    onChange={(e) => update({ imapHost: e.target.value })}
                  />
                </label>
                <label>
                  端口
                  <input
                    required
                    type="number"
                    min={1}
                    max={65535}
                    value={editing.imapPort}
                    onChange={(e) => update({ imapPort: +e.target.value })}
                  />
                </label>
                <label>
                  连接安全
                  <GlassSelect
                    value={editing.imapSecurity}
                    disabled={busy}
                    options={securities}
                    onValueChange={(value) =>
                      update({
                        imapSecurity: value as MailAccount["imapSecurity"],
                        imapPort: value === "tls" ? 993 : 143,
                      })
                    }
                  />
                </label>
              </div>
              <div className="two-fields">
                <label>
                  收信用户名
                  <input
                    value={editing.username}
                    placeholder="默认使用邮箱地址"
                    onChange={(e) => update({ username: e.target.value })}
                  />
                </label>
                <label>
                  密码 / 授权码
                  <input
                    type="password"
                    autoComplete="new-password"
                    value={imapSecret}
                    required={!editing.revision}
                    placeholder={
                      editing.revision ? "留空保留现有凭据" : "填写邮箱授权码"
                    }
                    onChange={(e) => setImapSecret(e.target.value)}
                  />
                </label>
              </div>
              <h3 className="mail-form-divider">发信服务器 · SMTP</h3>
              <div className="mail-server-fields">
                <label>
                  服务器
                  <input
                    required
                    value={editing.smtpHost}
                    placeholder="smtp.example.com"
                    onChange={(e) => update({ smtpHost: e.target.value })}
                  />
                </label>
                <label>
                  端口
                  <input
                    required
                    type="number"
                    min={1}
                    max={65535}
                    value={editing.smtpPort}
                    onChange={(e) => update({ smtpPort: +e.target.value })}
                  />
                </label>
                <label>
                  连接安全
                  <GlassSelect
                    value={editing.smtpSecurity}
                    disabled={busy}
                    options={securities}
                    onValueChange={(value) =>
                      update({
                        smtpSecurity: value as MailAccount["smtpSecurity"],
                        smtpPort: value === "tls" ? 465 : 587,
                      })
                    }
                  />
                </label>
              </div>
              <div className="two-fields">
                <label>
                  发信用户名
                  <input
                    value={editing.smtpUsername}
                    placeholder="默认与收信用户名相同"
                    onChange={(e) => update({ smtpUsername: e.target.value })}
                  />
                </label>
                <label>
                  发信密码 / 授权码
                  <input
                    type="password"
                    autoComplete="new-password"
                    value={smtpSecret}
                    placeholder="新邮箱留空使用收信凭据"
                    onChange={(e) => setSmtpSecret(e.target.value)}
                  />
                </label>
              </div>
              <div className="two-fields">
                <label>
                  自动检查间隔
                  <GlassSelect
                    disabled={busy}
                    value={String(editing.syncIntervalSeconds)}
                    options={[
                      { value: "60", label: "1 分钟（推荐）" },
                      { value: "180", label: "3 分钟" },
                      { value: "300", label: "5 分钟" },
                      { value: "900", label: "15 分钟" },
                    ]}
                    onValueChange={(value) =>
                      update({ syncIntervalSeconds: +value })
                    }
                  />
                </label>
                <label className="check-label">
                  <input
                    type="checkbox"
                    checked={editing.enabled}
                    onChange={(e) => update({ enabled: e.target.checked })}
                  />
                  启用邮箱与后台检查
                </label>
              </div>
              <p className="muted">
                请先在邮箱服务中开启 IMAP /
                SMTP。开启双重验证的邮箱通常需要应用专用密码或授权码。
              </p>
              {error && (
                <p role="alert" className="error">
                  {error}
                </p>
              )}
              <footer>
                {editing.revision && (
                  <button
                    className="danger"
                    type="button"
                    onClick={async () => {
                      if (
                        !confirm(
                          "移除此邮箱及本地缓存？服务器中的邮件不会删除。",
                        )
                      )
                        return;
                      setBusy(true);
                      try {
                        await command("mail_account_delete", {
                          accountId: editing.id,
                          revision: editing.revision,
                        });
                        await client.invalidateQueries({ queryKey: ["mail"] });
                        setEditing(null);
                      } catch (e) {
                        setError((e as Error).message);
                      } finally {
                        setBusy(false);
                      }
                    }}
                  >
                    <Trash2 size={15} />
                    移除邮箱
                  </button>
                )}
                <button type="submit" className="primary">
                  {busy ? "正在处理…" : "保存邮箱"}
                </button>
              </footer>
            </fieldset>
          </form>
        </Modal>
      )}
    </section>
  );
}
