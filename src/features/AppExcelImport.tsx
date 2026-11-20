import { useLayoutEffect, useRef, useState, type DragEvent, type ReactNode } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Download } from "lucide-react";
import Modal from "../components/Modal";
import { api, native } from "../lib/api";
import type { AppExcelSource, PageProps } from "../lib/types";
import "./app-excel-import.css";

type Busy = "template" | "preview" | "import";
type ExcelPreview<Row> = { rows: Row[]; errors: { row: number; message: string }[] };

interface Props extends Pick<PageProps, "refresh" | "notify"> {
  workspaceId: string;
  onClose: () => void;
}

interface ExcelImportProps<Row, Result> extends Pick<Props, "notify" | "onClose"> {
  title: string;
  className?: string;
  previewLabel: string;
  previewColumns: { label: string; render: (row: Row) => ReactNode }[];
  template: { name: string; file: string; save: (path: string) => Promise<void>; description?: string };
  previewSource: (source: AppExcelSource) => Promise<ExcelPreview<Row>>;
  importSource: (source: AppExcelSource) => Promise<Result>;
  summarize: (result: Result) => string | Promise<string>;
  confirmEnabled?: boolean;
  protectErrors?: boolean;
}

export default function AppExcelImport({ workspaceId, refresh, notify, onClose }: Props) {
  return (
    <ExcelImportDialog
      title="导入 Excel"
      previewLabel="应用记录"
      previewColumns={[
        { label: "名称", render: (row) => row.name },
        { label: "链接地址", render: (row) => row.url },
        { label: "分类", render: (row) => row.category },
        { label: "描述信息", render: (row) => row.description },
      ]}
      template={{
        name: "应用导入模板.xlsx",
        file: "applications.xlsx",
        save: api.saveAppExcelTemplate,
        description: "新版模板新增「账号」和「密码」两列，均可留空。",
      }}
      previewSource={api.previewAppExcel}
      importSource={(source) => api.importAppExcel(workspaceId, source)}
      summarize={async (result) => {
        const summary = `已创建 ${result.created} 个应用，跳过 ${result.skipped} 个，新增 ${result.categoriesCreated} 个分类。`;
        try {
          const refreshed = await refresh();
          if (!(refreshed && typeof refreshed === "object" && "isError" in refreshed && refreshed.isError === true)) return summary;
        } catch {
          // 导入完成后的刷新失败单独提示。
        }
        return `${summary} 应用列表刷新失败。`;
      }}
      notify={notify}
      onClose={onClose}
    />
  );
}

export function ExcelImportDialog<Row extends { row: number }, Result>({
  title,
  className,
  previewLabel,
  previewColumns,
  template,
  previewSource,
  importSource,
  summarize,
  confirmEnabled = true,
  protectErrors = false,
  notify,
  onClose,
}: ExcelImportProps<Row, Result>) {
  const [fileName, setFileName] = useState("");
  const [preview, setPreview] = useState<ExcelPreview<Row> | null>(null);
  const selectedSource = useRef<AppExcelSource | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<Busy | null>(null);
  const [dragging, setDragging] = useState(false);
  const operation = useRef<Busy | "idle" | "closed">("closed");
  const canImport = native && confirmEnabled && !busy && !!preview?.rows.length && !preview.errors.length;

  useLayoutEffect(() => {
    operation.current = "idle";
    return () => {
      operation.current = "closed";
      selectedSource.current = null;
    };
  }, []);

  function dismiss() {
    operation.current = "closed";
    selectedSource.current = null;
    onClose();
  }

  function errorDetail(cause: unknown) {
    return protectErrors ? "" : cause instanceof Error ? cause.message : String(cause);
  }

  function clearSelection() {
    setPreview(null);
    selectedSource.current = null;
  }

  async function saveTemplate() {
    if (!native || operation.current !== "idle") return;
    operation.current = "template";
    setBusy("template");
    try {
      const path = await save({
        defaultPath: template.name,
        filters: [{ name: "Excel", extensions: ["xlsx"] }],
      });
      if (!path || operation.current !== "template") return;
      setError("");
      await template.save(path);
      if (operation.current === "template") notify("已保存 Excel 模板。");
    } catch (cause) {
      if (operation.current === "template") {
        setError(`模板保存失败。${errorDetail(cause)}`);
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
      clearSelection();
      setError("");
    }
    try {
      const source = await readSource();
      if (!source || operation.current !== "preview") return;
      selectedFileName = source.fileName;
      setFileName(selectedFileName);
      clearSelection();
      setError("");
      const result = await previewSource(source);
      if (operation.current === "preview") {
        selectedSource.current = source;
        setPreview(result);
      }
    } catch (cause) {
      if (operation.current === "preview") {
        setFileName(selectedFileName);
        clearSelection();
        setError(`无法预览 Excel。${errorDetail(cause)}`);
      }
    } finally {
      if (operation.current === "preview") {
        operation.current = "idle";
        setBusy(null);
      }
    }
  }

  async function submit() {
    const source = selectedSource.current;
    if (!canImport || !source || operation.current !== "idle") return;
    operation.current = "import";
    setBusy("import");
    setError("");

    let result;
    try {
      result = await importSource(source);
    } catch (cause) {
      if (operation.current === "import") {
        setError(`导入失败。${errorDetail(cause)}`);
        operation.current = "idle";
        setBusy(null);
      }
      return;
    }

    // 工作空间切换或离开页面后，旧弹窗跳过结果提示。
    if (operation.current !== "import") return;
    const summary = await summarize(result);
    if (operation.current !== "import") return;
    notify(summary);
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
      clearSelection();
      setError("请选择一个 .xls 或 .xlsx 文件。");
      return;
    }
    void selectSource(() => api.readAppExcelFile(file), file.name);
  }

  function close() {
    if (operation.current !== "import") dismiss();
  }

  return (
    <Modal title={title} wide busy={busy === "import"} className={`app-excel-import ${className ?? ""}`} onClose={close}>
      <div className="app-excel-template">
        {native ? (
          <button type="button" disabled={busy !== null} onClick={() => void saveTemplate()}>
            <Download size={16} aria-hidden="true" />
            {busy === "template" ? "正在保存模板…" : "下载 Excel 模板"}
          </button>
        ) : (
          <a className="app-excel-template-download" href={`${import.meta.env.BASE_URL}templates/${template.file}`} download={template.name}>
            <Download size={16} aria-hidden="true" />
            下载 Excel 模板
          </a>
        )}
        {template.description && <span className="muted">{template.description}</span>}
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
              <table aria-label={previewLabel}>
                <thead>
                  <tr>
                    <th scope="col">行号</th>
                    {previewColumns.map(({ label }) => <th key={label} scope="col">{label}</th>)}
                  </tr>
                </thead>
                <tbody>
                  {preview.rows.map((row) => (
                    <tr key={row.row}>
                      <td>{row.row}</td>
                      {previewColumns.map(({ label, render }) => <td key={label}>{render(row)}</td>)}
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
