import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Archive, CloudUpload, Download, FolderOpen, LoaderCircle, RefreshCw, RotateCcw, Upload } from "lucide-react";
import Modal from "../components/Modal";
import { api, command, native } from "../lib/api";
import type { BackupConfig, BackupProgress, BackupProgressEvent, BackupRecord, BackupSaveInput, BackupState } from "../lib/backup-types";
import type { StorageInfo } from "../lib/types";
import "./backup-settings.css";

const statusLabels: Record<BackupRecord["status"], string> = {
  preparing: "准备中", packing: "打包中", verifying: "核验中",
  uploading: "上传中", success: "成功", failed: "失败", unknown: "待确认",
};
const progressLabels: Record<BackupProgress["phase"], string> = {
  preparing: "准备备份", database: "备份数据库", packing: "打包文件", hashing: "计算文件校验值",
  uploading: "上传至云端", verifying: "等待云端确认", cleaning: "清理过期备份", completed: "备份完成",
};
function activeRecord(record: BackupRecord) {
  return ["preparing", "packing", "uploading", "verifying"].includes(record.status);
}
function BackupProgressBar({ progress }: { progress: BackupProgress }) {
  const measurable = !!progress.total && !["preparing", "verifying", "cleaning"].includes(progress.phase);
  const percent = measurable ? Math.min(100, Math.floor(progress.completed / progress.total! * 100)) : undefined;
  const label = progressLabels[progress.phase];
  return <div className="backup-progress" data-phase={progress.phase}>
    <div className="backup-progress-caption"><span><LoaderCircle size={13} className="backup-spinner" />{label}</span><span>{percent !== undefined ? `${percent}%` : "进行中"}</span></div>
    <progress max={100} value={percent} aria-label={label} />
    {measurable && <small>{progress.phase === "database" ? `${progress.completed} / ${progress.total} 页` : `${sizeLabel(progress.completed)} / ${sizeLabel(progress.total!)}`}</small>}
    {progress.phase === "verifying" && <small>数据已发送，正在等待云端确认，请勿重复提交。</small>}
  </div>;
}
const triggerLabels: Record<BackupRecord["trigger"], string> = {
  manual: "手动", scheduled: "定时", remote: "远端记录",
};
function sizeLabel(size: number) {
  if (!size) return "—";
  return size < 1024 * 1024 ? `${(size / 1024).toFixed(1)} KB` : `${(size / 1024 / 1024).toFixed(1)} MB`;
}
function dateLabel(date?: string | null) {
  if (!date) return "—";
  const value = new Date(date);
  return Number.isNaN(value.getTime()) ? "—" : value.toLocaleString("zh-CN", { hour12: false });
}

export default function BackupSettings({ info, onRestored, notify }: {
  info: StorageInfo;
  onRestored: () => Promise<unknown>;
  notify: (message: string) => void;
}) {
  const [state, setState] = useState<BackupState | null>(null);
  const [form, setForm] = useState<BackupConfig | null>(null);
  const [accessKeyId, setAccessKeyId] = useState("");
  const [secretAccessKey, setSecretAccessKey] = useState("");
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [restoreRecord, setRestoreRecord] = useState<BackupRecord | null>(null);
  const [pendingRecord, setPendingRecord] = useState<BackupRecord | null>(null);
  const [dirty, setDirty] = useState(false);
  const dirtyRef = useRef(false);
  const inFlight = useRef(false);
  const mounted = useRef(false);
  const hasSnapshot = useRef(false);
  const version = useRef(0);

  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    const unlisteners: (() => void)[] = [];
    function remember(stop: () => void) { if (cancelled) stop(); else unlisteners.push(stop); }
    async function reload() {
      if (inFlight.current) return;
      const request = ++version.current;
      try {
        const next = await api.backupState();
        if (cancelled || request !== version.current) return;
        hasSnapshot.current = true;
        setState(next);
        // 原生定时任务只刷新运行状态与记录，保留正在编辑的配置。
        if (!dirtyRef.current) setForm(next.config);
      } catch (cause) {
        if (!cancelled && request === version.current) setError(String(cause instanceof Error ? cause.message : cause));
      }
    }
    if (native) {
      void reload();
      void listen("backup-updated", () => { void reload(); })
        .then(remember)
        .catch(() => { if (!cancelled) setError("备份状态监听失败，请重新打开此设置页。"); });
      void listen<BackupProgressEvent>("backup-progress", ({ payload }) => {
        if (cancelled || !payload || payload.workspaceId !== info.workspaceId || payload.dataRoot !== info.dataRoot) return;
        // 进度事件直接更新记录，不受当前操作 inFlight 限制，也不等待数据库锁。
        if (hasSnapshot.current) ++version.current;
        setPendingRecord((record) => record?.id === payload.record.id ? null : record);
        setState((current) => {
          if (!current) return current;
          const records = current.records.some((record) => record.id === payload.record.id)
            ? current.records.map((record) => record.id === payload.record.id ? payload.record : record)
            : [payload.record, ...current.records];
          return { ...current, records, running: records.some(activeRecord) };
        });
      }).then(remember).catch(() => { if (!cancelled) setError("备份进度监听失败，请重新打开此设置页再备份。"); });
    }
    return () => { cancelled = true; mounted.current = false; hasSnapshot.current = false; ++version.current; unlisteners.forEach((stop) => stop()); };
  }, [info.workspaceId, info.dataRoot]);

  function markDirty() { dirtyRef.current = true; setDirty(true); setMessage(""); }
  function update(patch: Partial<BackupConfig>) {
    markDirty();
    setForm((current) => current ? { ...current, ...patch } : current);
  }
  function input(): BackupSaveInput {
    if (!form) throw new Error("备份配置尚未加载");
    return { ...form, accessKeyId: accessKeyId.trim() || undefined, secretAccessKey: secretAccessKey || undefined };
  }
  function accept(next: BackupState, saved = false) {
    if (!mounted.current) return;
    hasSnapshot.current = true;
    setState(next);
    if (saved || !dirtyRef.current) setForm(next.config);
    if (saved) {
      dirtyRef.current = false;
      setDirty(false);
      setAccessKeyId("");
      setSecretAccessKey("");
    }
  }
  async function act(label: string, action: () => Promise<void>, success?: string) {
    if (inFlight.current) return;
    inFlight.current = true;
    ++version.current;
    setBusy(label); setError(""); setMessage("");
    try {
      await action();
      if (mounted.current && success) setMessage(success);
    } catch (cause) {
      if (mounted.current) setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      // 失败也回查持久化记录，上传结果不明不能显示为成功。
      if (mounted.current && native) {
        try { accept(await api.backupState()); } catch { /* 保留本次操作的具体错误。 */ }
      }
      inFlight.current = false;
      if (mounted.current) setBusy("");
    }
  }
  async function saveConfig() { accept(await api.backupSave(input()), true); }
  async function workspaceChanged() {
    // 即使外层刷新失败，也不能把恢复前的密钥草稿带到新工作空间。
    dirtyRef.current = false;
    setDirty(false);
    setAccessKeyId("");
    setSecretAccessKey("");
    setForm(null);
    setState(null);
    accept(await api.backupState(), true);
    await onRestored();
  }
  async function createBackup() {
    const id = crypto.randomUUID();
    setPendingRecord({ id, key: "", filename: "正在准备备份…", size: 0, createdAt: new Date().toISOString(), trigger: "manual", status: "preparing", monthlyArchive: false, progress: { phase: "preparing", completed: 0 } });
    try {
      if (dirtyRef.current) await saveConfig();
      const next = await api.backupCreate(id);
      accept(next);
      const record = next.records.find((record) => record.id === id);
      if (record && record.status !== "success") throw new Error(record.error || "上传结果尚未确认，请点击该记录的“核验结果”。");
    } finally {
      if (mounted.current) setPendingRecord(null);
    }
  }
  async function verifyRecord(id: string) {
    const next = await api.backupVerify(id);
    accept(next);
    const record = next.records.find((record) => record.id === id);
    if (record?.status === "success") setMessage("已确认云端文件存在且大小一致，可以还原。");
    else if (record?.status === "failed") setMessage(record.retryAvailable ? "云端未找到完整备份，请点击“重新上传”。" : "云端未找到完整备份，本地备份文件已清理，请点击“重新备份”。");
    else throw new Error(record?.error || "暂时无法确认远端状态，请稍后再次核验。");
  }
  async function retryRecord(id: string) {
    const next = await api.backupRetry(id);
    accept(next);
    const record = next.records.find((record) => record.id === id);
    if (record?.status !== "success") throw new Error(record?.error || "上传结果尚未确认，请点击“核验结果”。");
    setMessage("已确认备份上传成功，可以还原。");
  }
  async function exportLocal() {
    const destination = await save({ defaultPath: `perch-${new Date().toISOString().slice(0, 10)}.zip`, filters: [{ name: "栖点备份", extensions: ["zip"] }] });
    if (!destination) return;
    await command("data_backup", { destination });
    if (mounted.current) setMessage("完整备份已导出");
  }
  async function downloadRecord(record: BackupRecord) {
    const destination = await save({ title: "下载云端备份", defaultPath: record.filename, filters: [{ name: "栖点备份", extensions: ["zip"] }] });
    if (!destination) return;
    await api.backupDownload(record.id, destination);
    if (mounted.current) setMessage(`备份已下载到 ${destination}`);
  }
  async function restoreLocal() {
    const archive = await open({ filters: [{ name: "栖点备份", extensions: ["zip"] }] });
    if (typeof archive !== "string") return;
    const target = await open({ directory: true, title: "选择空目录用于恢复" });
    if (typeof target !== "string" || !confirm("验证备份并恢复到所选空目录？验证成功后切换，当前数据保留。")) return;
    await command("data_restore", { archive, target });
    await workspaceChanged();
    notify("备份已还原，原数据目录已保留");
  }
  const disabled = !!busy || !!state?.running;
  const configured = !!state?.config.hasCredentials;
  const records = pendingRecord ? [pendingRecord, ...(state?.records || []).filter((record) => record.id !== pendingRecord.id)] : state?.records || [];
  const creating = busy === "创建备份" || records.some(activeRecord);

  return <div className="backup-settings">
    <section className="backup-local" aria-label="本地数据">
      <div className="backup-section-heading">
        <div><h2>本地数据</h2><p className="path-text">{info.dataRoot}</p></div>
        <button disabled={!native || disabled} onClick={() => void revealItemInDir(info.dataRoot).catch((cause) => notify(String(cause)))}><FolderOpen size={15} />打开目录</button>
      </div>
      <div className="backup-local-actions">
        <button disabled={!native || disabled} onClick={() => void act("导出本地备份", exportLocal)}><Download size={15} />导出完整备份</button>
        <button disabled={!native || disabled} onClick={() => void act("还原本地备份", restoreLocal)}><Upload size={15} />从文件还原</button>
        <button disabled={!native || disabled} onClick={() => void act("迁移数据", async () => {
          const target = await open({ directory: true });
          if (typeof target !== "string" || !confirm("将数据复制到所选空目录并切换？原目录保留。")) return;
          await command("storage_move", { target }); await workspaceChanged(); notify("数据目录已迁移");
        })}>迁移数据目录</button>
        <button disabled={!native || disabled || info.dataRoot === info.defaultRoot} onClick={() => void act("恢复默认目录", async () => {
          if (!confirm("迁移回默认目录？存在旧数据时将拒绝覆盖。")) return;
          await command("storage_move", { target: info.defaultRoot }); await workspaceChanged(); notify("已恢复默认目录");
        })}>恢复默认路径</button>
      </div>
      <p className="backup-hint">备份包含数据库、加密配置与文档原件。本机模型、主密钥和独立登录凭据不随备份导出。跨机还原需要原本机主密钥。</p>
    </section>

    {!native && <p className="backup-feedback" role="status">请在栖点桌面应用中配置 S3 存储和备份。浏览器预览不保存业务数据。</p>}
    {native && !form && !error && <p role="status">正在加载备份配置…</p>}
    {error && <p className="backup-feedback error" role="alert">{error}</p>}
    {message && <p className="backup-feedback backup-success" role="status">{message}</p>}
    {(busy || state?.running) && <p className="backup-feedback" role="status"><LoaderCircle size={14} className="backup-spinner" />{busy || "正在备份"}，请勿退出应用。</p>}

    {form && <>
      <form className="backup-config" onSubmit={(event) => { event.preventDefault(); void act("保存配置", saveConfig, "备份配置已保存"); }}>
        <fieldset disabled={disabled} className="backup-form-reset">
          <section className="backup-s3" aria-label="S3 存储配置">
            <div className="backup-section-heading"><div><h2>S3 存储配置</h2><p className="backup-hint">支持 AWS S3、阿里云 OSS、Cloudflare R2、MinIO 等兼容服务。</p></div><span className="backup-credential-state">{configured ? "凭据已加密保存" : "尚未配置凭据"}</span></div>
            <div className="form-grid backup-fields">
              <label className="backup-endpoint">端点地址<input type="url" required value={form.endpoint} placeholder="https://s3.example.com" onChange={(e) => update({ endpoint: e.target.value })} /></label>
              <label>区域（Region）<input required value={form.region} placeholder="us-east-1 / auto" onChange={(e) => update({ region: e.target.value })} /></label>
              <label>存储桶（Bucket）<input required value={form.bucket} placeholder="perch-backups" onChange={(e) => update({ bucket: e.target.value })} /></label>
              <label className="backup-prefix">上传目录（Key 前缀）<input value={form.prefix} placeholder="backups/perch/" onChange={(e) => update({ prefix: e.target.value })} /><small>在此目录下按工作空间分开保存；留空使用存储桶根目录。</small></label>
              <label>Access Key ID<input autoComplete="off" value={accessKeyId} placeholder={configured ? "已保存，留空保留" : "填写访问密钥 ID"} onChange={(e) => { markDirty(); setAccessKeyId(e.target.value); }} /></label>
              <label className="backup-secret">Secret Access Key<input type="password" autoComplete="new-password" value={secretAccessKey} placeholder={configured ? "已保存，留空保留" : "填写访问密钥"} onChange={(e) => { markDirty(); setSecretAccessKey(e.target.value); }} /></label>
            </div>
            <div className="backup-inline-options"><label className="check-label"><input type="checkbox" checked={form.forcePathStyle} onChange={(e) => update({ forcePathStyle: e.target.checked })} />强制路径风格</label><span className="backup-hint">MinIO 等自建存储可启用；通常使用服务商默认风格。</span></div>
            {form.endpoint.trim().startsWith("http:") && <p className="backup-warning">当前使用 HTTP，备份内容将通过未加密连接传输。公网存储请使用 HTTPS。</p>}
          </section>
          <section className="backup-schedule" aria-label="定时备份">
            <div className="backup-section-heading"><div><h2>定时备份</h2><p className="backup-hint">按本机时区执行；栖点留在托盘时仍可备份。</p></div><label className="check-label"><input type="checkbox" checked={form.enabled} onChange={(e) => update({ enabled: e.target.checked })} />启用定时备份</label></div>
            <div className="form-grid backup-schedule-fields">
              <label>Cron 表达式<input required value={form.cron} spellCheck={false} placeholder="0 2 * * *" onChange={(e) => update({ cron: e.target.value })} /><small>分 时 日 月 周，例如 0 2 * * * 表示每天凌晨 2 点。</small></label>
              <label>保留天数<input type="number" min={0} max={36500} required value={form.retentionDays} onChange={(e) => update({ retentionDays: Number(e.target.value) })} /><small>0 表示不按天数清理</small></label>
              <label>最大保留份数<input type="number" min={0} max={10000} required value={form.maxBackups} onChange={(e) => update({ maxBackups: Number(e.target.value) })} /><small>0 表示不按份数清理</small></label>
            </div>
            <div className="backup-inline-options"><label className="check-label"><input type="checkbox" checked={form.monthlyArchive} onChange={(e) => update({ monthlyArchive: e.target.checked })} />保留月度归档</label><span className="backup-hint">每月首份成功备份长期保留，不受普通备份清理规则影响。</span></div>
            <p className="backup-hint">成功备份后按以上任一条件清理本工作空间的普通备份；其他文件不受影响。退出期间不执行；休眠唤醒后最多补执行一次。</p>
            <p className="backup-next-run">下次备份：{state?.config.enabled ? dateLabel(state.nextRunAt) : "未启用"}{dirty && <span className="backup-hint"> · 配置有未保存修改</span>}</p>
          </section>
          <div className="backup-save-bar"><span className="backup-hint">更换端点、区域或存储桶后，需重新填写两项密钥。</span><div className="actions"><button type="button" disabled={!form.endpoint || !form.bucket} onClick={() => void act("测试连接", async () => { await api.backupTest(input()); }, "连接正常，已确认可以读取此备份目录；上传权限将在备份时验证。")}>测试连接</button><button type="submit" className="primary">保存配置</button></div></div>
        </fieldset>
      </form>

      <section className="backup-records" aria-label="备份记录">
        <div className="backup-section-heading"><div><h2>备份记录 <span className="backup-record-count">{records.length}</span></h2><p className="backup-hint">成功备份保存在 S3，可下载到本地或还原。</p></div><div className="actions"><button disabled={disabled || !configured} onClick={() => void act("刷新备份记录", async () => { accept(await api.backupRefresh()); })}><RefreshCw size={14} />刷新</button><button className="primary" disabled={disabled || (!configured && !accessKeyId)} onClick={() => void act("创建备份", createBackup, "备份已上传至 S3") }>{creating ? <LoaderCircle size={15} className="backup-spinner" /> : <CloudUpload size={15} />}{creating ? "正在备份…" : "立即备份"}</button></div></div>
        {records.some((record) => record.status === "unknown") && <p className="backup-unknown-help">“待确认”表示没有收到明确的上传结果，文件可能已经在云端。点击“核验结果”检查；确认未上传后可手动重新上传。</p>}
        {!records.length ? <div className="backup-empty"><Archive size={26} /><strong>还没有备份记录</strong><span>保存 S3 配置后，点击“立即备份”创建第一份备份。</span></div> : <div className="backup-table-wrap"><table className="backup-table"><thead><tr><th>备份文件 / 进度</th><th>状态</th><th>大小</th><th>触发方式</th><th>操作</th></tr></thead><tbody>{records.map((record) => <tr key={record.id} data-backup-id={record.id}>
          <td><div className="backup-filename" title={record.key}>{record.filename || "未生成备份文件"}</div><div className="backup-hint">{dateLabel(record.createdAt)}{record.monthlyArchive && <span className="backup-archive">月度归档</span>}</div>{activeRecord(record) && <BackupProgressBar progress={record.progress || { phase: "preparing", completed: 0 }} />}{record.error && <div className={`backup-record-error ${record.status === "unknown" ? "backup-uncertain" : ""}`}>{record.error}</div>}</td>
          <td><span className={`backup-status ${record.status}`}>{activeRecord(record) && <LoaderCircle size={12} className="backup-spinner" />}{statusLabels[record.status]}</span></td><td>{sizeLabel(record.size)}</td><td>{triggerLabels[record.trigger]}</td><td><div className="backup-record-actions">
            {record.status === "unknown" && <button disabled={disabled || dirty} onClick={() => void act("核验上传结果", () => verifyRecord(record.id))}><RefreshCw size={13} />核验结果</button>}
            {record.status === "failed" && (record.retryAvailable ? <button disabled={disabled || dirty} onClick={() => void act("重新上传备份", () => retryRecord(record.id))}><Upload size={13} />重新上传</button> : <button disabled={disabled || dirty} onClick={() => void act("创建备份", createBackup, "备份已上传至 S3")}><CloudUpload size={13} />重新备份</button>)}
            {record.status === "success" && <button disabled={disabled || dirty} title={dirty ? "请先保存配置再下载" : "下载云端备份到本地"} onClick={() => void act(`下载备份:${record.id}`, () => downloadRecord(record))}>{busy === `下载备份:${record.id}` ? <LoaderCircle size={14} className="backup-spinner" /> : <Download size={14} />}{busy === `下载备份:${record.id}` ? "下载中…" : "下载"}</button>}
            {record.status === "success" && <button disabled={disabled || dirty} title={dirty ? "请先保存配置再还原" : "从此备份还原"} onClick={() => { setError(""); setRestoreRecord(record); }}><RotateCcw size={14} />还原</button>}
          </div></td>
        </tr>)}</tbody></table></div>}
      </section>
    </>}
    {restoreRecord && <Modal title="还原这份备份？" busy={!!busy} onClose={() => setRestoreRecord(null)}>
      <div className="backup-restore-summary"><strong>{restoreRecord.filename}</strong><p>{dateLabel(restoreRecord.createdAt)} · {sizeLabel(restoreRecord.size)}</p></div>
      <p>下载并校验成功后，自动还原到新的数据目录并切换。当前数据目录会保留，可通过“打开目录”查看还原后的数据。</p>
      <p className="backup-hint">请先保存其他页面的修改。加密配置需要原本机主密钥，登录凭据不包含在备份中。</p>
      {error && <p className="error" role="alert">{error}</p>}
      <div className="backup-modal-actions"><button disabled={!!busy} onClick={() => setRestoreRecord(null)}>取消</button><button className="primary" disabled={!!busy} onClick={() => void act("还原备份", async () => {
        await api.backupRestore(restoreRecord.id);
        setRestoreRecord(null);
        await workspaceChanged();
        notify("备份已还原，原数据目录已保留");
      })}>{busy ? "正在还原…" : "确认还原"}</button></div>
    </Modal>}
  </div>;
}
