import { useRef, useState, type FormEvent } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Pencil, Plus, Trash2 } from "lucide-react";
import Modal from "../components/Modal";
import { api } from "../lib/api";
import type { SharedTableGroup, SharedTableGroupInput } from "../lib/types";

type GroupAction =
  | { kind: "save"; input: SharedTableGroupInput }
  | { kind: "delete"; group: SharedTableGroup };

interface GroupManagerProps {
  workspaceId: string;
  queryKey: readonly ["shared-table-groups", string, string];
  groups: SharedTableGroup[];
  onClose: () => void;
  onDelete: (id: string) => void;
  notify: (message: string) => void;
}

export default function SharedTableGroupManager({
  workspaceId,
  queryKey,
  groups,
  onClose,
  onDelete,
  notify,
}: GroupManagerProps) {
  const client = useQueryClient();
  const [action, setAction] = useState<GroupAction | null>(null);
  const [error, setError] = useState("");
  const submitting = useRef(false);
  const { mutate, isPending: busy } = useMutation({
    mutationFn: async (request: GroupAction) => {
      if (request.kind === "save") return api.saveSharedTableGroup(workspaceId, request.input);
      await api.deleteSharedTableGroup(workspaceId, request.group.id, request.group.revision);
      return null;
    },
    onSuccess: async (saved, request) => {
      await client.cancelQueries({ queryKey, exact: true });
      client.setQueryData<SharedTableGroup[]>(queryKey, (current = []) => {
        if (request.kind === "delete") return current.filter((group) => group.id !== request.group.id);
        if (!saved) return current;
        return current.some((group) => group.id === saved.id)
          ? current.map((group) => group.id === saved.id ? saved : group)
          : [...current, saved];
      });
      void client.invalidateQueries({ queryKey, exact: true });
    },
  });

  function selectAction(next: GroupAction | null) {
    if (submitting.current) return;
    setAction(next);
    setError("");
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!action || submitting.current) return;
    let request = action;
    if (action.kind === "save") {
      const name = action.input.name.trim();
      if (!name) {
        setError("请输入分组名称。");
        return;
      }
      request = { kind: "save", input: { ...action.input, name } };
    }
    submitting.current = true;
    setError("");
    mutate(request, {
      onSuccess: (_, completed) => {
        if (completed.kind === "delete") onDelete(completed.group.id);
        setAction(null);
        notify(completed.kind === "delete" ? "分组已删除" : "分组已保存");
      },
      onError: (reason) => setError(reason.message),
      onSettled: () => { submitting.current = false; },
    });
  }

  return (
    <Modal title="管理分组" busy={busy} onClose={() => { if (!submitting.current) onClose(); }}>
      <div className="shared-table-groups">
        {groups.length === 0 ? <p className="muted">暂无分组</p> : (
          <ul className="shared-table-group-list" aria-label="分组列表">
            {groups.map((group) => (
              <li key={group.id}>
                <span>{group.name}</span>
                <button
                  type="button"
                  className="icon-button"
                  aria-label={`重命名分组 ${group.name}`}
                  title={`重命名分组 ${group.name}`}
                  disabled={busy}
                  onClick={() => selectAction({ kind: "save", input: { ...group } })}
                >
                  <Pencil size={15} aria-hidden="true" />
                </button>
                <button
                  type="button"
                  className="icon-button danger"
                  aria-label={`删除分组 ${group.name}`}
                  title={`删除分组 ${group.name}`}
                  disabled={busy}
                  onClick={() => selectAction({ kind: "delete", group })}
                >
                  <Trash2 size={15} aria-hidden="true" />
                </button>
              </li>
            ))}
          </ul>
        )}
        <button
          type="button"
          disabled={busy}
          onClick={() => selectAction({ kind: "save", input: { name: "" } })}
        >
          <Plus size={15} /> 新增分组
        </button>
        {action && (
          <form key={action.kind === "save" ? action.input.id ?? "new" : `delete-${action.group.id}`} className="form-grid" noValidate onSubmit={submit}>
            {action.kind === "save" ? (
              <label>
                分组名称
                <input
                  autoFocus
                  required
                  value={action.input.name}
                  disabled={busy}
                  onChange={(event) => setAction({
                    kind: "save",
                    input: { ...action.input, name: event.target.value },
                  })}
                />
              </label>
            ) : (
              <p className="shared-tables-confirm">确定删除分组「{action.group.name}」吗？</p>
            )}
            {error && <p className="error" role="alert">{error}</p>}
            <footer>
              <button
                type="button"
                autoFocus={action.kind === "delete"}
                disabled={busy}
                onClick={() => selectAction(null)}
              >
                取消
              </button>
              <button type="submit" className="primary" disabled={busy}>
                {action.kind === "delete" ? (busy ? "删除中…" : "确认删除分组") : (busy ? "保存中…" : "保存分组")}
              </button>
            </footer>
          </form>
        )}
      </div>
    </Modal>
  );
}
