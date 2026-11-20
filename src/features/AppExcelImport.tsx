import { useLayoutEffect, useRef, useState, type DragEvent } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Download } from "lucide-react";
import Modal from "../components/Modal";
import { api, native } from "../lib/api";
import type { AppExcelSource, PageProps } from "../lib/types";
import "./app-excel-import.css";

type Preview = Awaited<ReturnType<typeof api.previewAppExcel>>;
type Busy = "template" | "preview" | "import";

interface Props extends Pick<PageProps, "refresh" | "notify"> {
  workspaceId: string;
  onClose: () => void;
}

export default function AppExcelImport({ workspaceId, refresh, notify, onClose }: Props) {
  const [fileName, setFileName] = useState("");
  const [selection, setSelection] = useState<{ source: AppExcelSource; preview: Preview } | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<Busy | null>(null);
  const [dragging, setDragging] = useState(false);
  const operation = useRef<Busy | "idle" | "closed">("closed");
  const preview = selection?.preview;
  const canImport = native && !busy && !!preview?.rows.length && !preview.errors.length;

  useLayoutEffect(() => {
    operation.current = "idle";
    return () => { operation.current = "closed"; };
  }, []);

  function dismiss() {
    operation.current = "closed";
    onClose();
  }

  async function saveTemplate() {
    if (!native || operation.current !== "idle") return;
    operation.current = "template";
    setBusy("template");
    try {
      const path = await save({
        defaultPath: "应用导入模板.xlsx",
        filters: [{ name: "Excel", extensions: ["xlsx"] }],
      });
      if (!path || operation.current !== "template") return;
      setError("");
      await api.saveAppExcelTemplate(path);
      if (operation.current === "template") notify("已保存 Excel 模板。");
    } catch (cause) {
      if (operation.current === "template") {
        setError(`模板保存失败。${cause instanceof Error ? cause.message : String(cause)}`);
      }
    } finally {
      if (operation.current === "template") {
        operation.current = "idle";
        setBusy(null);
      }
    }
  }

  async function selectSource(readSource: () => Promise<AppExcelSource | null>, droppedFileName = "") {
    if (!native || operation.current !== "idle") return;
    operation.current = "preview";
    setBusy("preview");
    let selectedFileName = droppedFileName;
    if (droppedFileName) {
      setFileName(droppedFileName);
      setSelection(null);
      setError("");
    }
    try {
      const source = await readSource();
      if (!source || operation.current !== "preview") return;
      selectedFileName = source.fileName;
      setFileName(selectedFileName);
      setSelection(null);
      setError("");
      const result = await api.previewAppExcel(source);
      if (operation.current === "preview") setSelection({ source, preview: result });
    } catch (cause) {
      if (operation.current === "preview") {
        setFileName(selectedFileName);
        setSelection(null);
        setError(`无法预览 Excel。${cause instanceof Error ? cause.message : String(cause)}`);
      }
    } finally {
      if (operation.current === "preview") {
        operation.current = "idle";
        setBusy(null);
      }
    }
  }

  async function submit() {
    if (!canImport || !selection || operation.current !== "idle") return;
    operation.current = "import";
    setBusy("import");
    setError("");

    let result;
    try {
      result = await api.importAppExcel(workspaceId, selection.source);
    } catch (cause) {
      if (operation.current === "import") {
        setError(`导入失败。${cause instanceof Error ? cause.message : String(cause)}`);
        operation.current = "idle";
        setBusy(null);
      }
      return;
    }

    // 工作空间切换或离开页面后，旧请求不能刷新列表或关闭新弹窗。
    if (operation.current !== "import") return;
    const summary = `已创建 ${result.created} 个应用，跳过 ${result.skipped} 个，新增 ${result.categoriesCreated} 个分类。`;
    let refreshFailed = false;
    try {
      const refreshed = await refresh();
      refreshFailed = !!refreshed && typeof refreshed === "object" && "isError" in refreshed && refreshed.isError === true;
    } catch {
      refreshFailed = true;
    }
    if (operation.current !== "import") return;
    notify(refreshFailed ? `${summary} 应用列表刷新失败。` : summary);
    dismiss();
  }

  function dragOver(event: DragEvent<HTMLDivElement>) {
    if (!event.dataTransfer.types.includes("Files")) return;
    event.preventDefault();
    const available = operation.current === "idle";
    event.dataTransfer.dropEffect = available ? "copy" : "none";
    setDragging(available);
  }

  function dropFile(event: DragEvent<HTMLDivElement>) {
    event.preventDefault();
    setDragging(false);
    const files = event.dataTransfer.files;
    if (operation.current !== "idle" || !files.length) return;
    const file = files[0];
    if (files.length !== 1 || !/\.xlsx?$/i.test(file.name)) {
      setFileName("");
      setSelection(null);
      setError("请选择一个 .xls 或 .xlsx 文件。");
      return;
    }
    void selectSource(() => api.readAppExcelFile(file), file.name);
  }

  function close() {
    if (operation.current !== "import") dismiss();
  }

  return (
    <Modal title="导入 Excel" wide busy={busy === "import"} className="app-excel-import" onClose={close}>
      <div className="app-excel-template">
        {native ? (
          <button type="button" disabled={busy !== null} onClick={() => void saveTemplate()}>
            <Download size={16} aria-hidden="true" />
            {busy === "template" ? "正在保存模板…" : "下载 Excel 模板"}
          </button>
        ) : (
          <a className="app-excel-template-download" href={`${import.meta.env.BASE_URL}templates/applications.xlsx`} download="应用导入模板.xlsx">
            <Download size={16} aria-hidden="true" />
            下载 Excel 模板
          </a>
        )}
        <span className="muted">新版模板新增「账号」和「密码」两列，均可留空。</span>
      </div>
      {native ? (
        <>
          <div
            className="app-excel-drop"
            data-dragging={dragging || undefined}
            onDragOver={dragOver}
            onDragLeave={(event) => {
              const next = event.relatedTarget;
              if (!(next instanceof Node && event.currentTarget.contains(next))) setDragging(false);
            }}
            onDrop={dropFile}
          >
            <button type="button" disabled={busy !== null} onClick={() => void selectSource(() => api.pickAppExcel())}>
              {fileName ? "更换文件" : "选择文件"}
            </button>
            <span className="muted">或拖入一个 .xls / .xlsx 文件</span>
          </div>
          {(fileName || busy === "preview") && (
            <p className="app-excel-status" role="status">
              {fileName}
              {busy === "preview"
                ? `${fileName ? " · " : ""}正在处理文件…`
                : preview && (preview.rows.length ? ` · ${preview.rows.length} 条有效记录` : " · 没有可导入的记录")}
            </p>
          )}
          {error && <p className="error" role="alert">{error}</p>}
          {!!preview?.errors.length && (
            <div className="app-excel-errors error" role="alert">
              <p>请修正 Excel 中的问题后重新选择文件。</p>
              <ul>
                {preview.errors.map(({ row, message }, index) => (
                  <li key={`${row}-${index}`}>Excel 第 {row} 行：{message}</li>
                ))}
              </ul>
            </div>
          )}
          {!!preview?.rows.length && (
            <div className="table-wrap app-excel-preview" tabIndex={0} role="region" aria-label="Excel 导入预览">
              <table aria-label="应用记录">
                <thead>
                  <tr>
                    <th scope="col">行号</th>
                    <th scope="col">名称</th>
                    <th scope="col">链接地址</th>
                    <th scope="col">分类</th>
                    <th scope="col">描述信息</th>
                  </tr>
                </thead>
                <tbody>
                  {preview.rows.map((row) => (
                    <tr key={row.row}>
                      <td>{row.row}</td>
                      <td>{row.name}</td>
                      <td>{row.url}</td>
                      <td>{row.category}</td>
                      <td>{row.description}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      ) : <p className="notice app-excel-status">请在栖点桌面应用中解析和导入 Excel 文件。</p>}
      <footer className="modal-actions">
        <button type="button" disabled={busy === "import"} onClick={close}>{native ? "取消" : "关闭"}</button>
        {native && (
          <button type="button" className="primary" disabled={!canImport} onClick={() => void submit()}>
            {busy === "import" ? "导入中…" : "确认导入"}
          </button>
        )}
      </footer>
    </Modal>
  );
}
