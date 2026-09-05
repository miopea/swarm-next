import { useId, useRef, useState, type FormEvent } from "react";
import { changeTaskPrerequisite, type Task } from "../api/tasks";
import { RuntimeRequestError } from "../api/request";
import { useModalFocus } from "../shared/useModalFocus";
import UnsavedChangesPrompt from "../shared/UnsavedChangesPrompt";

export default function TaskPrerequisiteDialog({ task, candidates, operatorToken, onChanged, onClose }: {
  task: Task;
  candidates: Task[];
  operatorToken: string;
  onChanged: (updated: Task) => void;
  onClose: () => void;
}) {
  const id = useId();
  const [operation, setOperation] = useState<"add" | "remove">(task.state === "blocked" ? "add" : "remove");
  const [query, setQuery] = useState("");
  const [target, setTarget] = useState("");
  const [reason, setReason] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [confirmClose, setConfirmClose] = useState(false);
  const submitting = useRef(false);
  const dirty = Boolean(target || reason.trim());
  function requestClose() {
    if (submitting.current) return;
    if (confirmClose) return setConfirmClose(false);
    if (dirty) return setConfirmClose(true);
    onClose();
  }
  const dialog = useModalFocus<HTMLElement>(requestClose);
  const existing = task.prerequisites ?? [];
  const choices = operation === "remove"
    ? existing.map((item) => ({ id: item.prerequisite_id, title: item.title, state: item.removed ? "removed" : item.state }))
    : candidates.filter((item) => item.id !== task.id && item.hive_id === task.hive_id
      && !existing.some((edge) => edge.prerequisite_id === item.id));
  const matching = choices.filter((item) => `${item.title} ${item.id}`.toLowerCase().includes(query.trim().toLowerCase()));
  const visible = matching.slice(0, 50);
  const selected = choices.find((item) => item.id === target);
  if (selected && !visible.some((item) => item.id === target)) visible.unshift(selected);
  const reasonTooLong = new TextEncoder().encode(reason.trim()).length > 2048;
  const canAdd = task.state === "blocked" && existing.length < 32;
  const valid = Boolean(selected && reason.trim() && !reasonTooLong && (operation === "remove" || canAdd));
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!valid || submitting.current || confirmClose) return;
    submitting.current = true;
    setSaving(true);
    setError("");
    try {
      const updated = await changeTaskPrerequisite(operatorToken, task.id, {
        prerequisite_id: target, operation, reason: reason.trim(),
      });
      onChanged(updated);
      onClose();
    } catch (failure) {
      setError(failure instanceof RuntimeRequestError && failure.status === 409
        ? `Swarm refused this change. ${failure.message}`
        : "The change could not be confirmed. Your choices are still here; check the task before retrying.");
    } finally {
      submitting.current = false;
      setSaving(false);
    }
  }
  return <div className="task-detail-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) requestClose(); }}>
    <section ref={dialog} tabIndex={-1} className="task-detail-dialog task-prerequisite-dialog" role="dialog" aria-modal="true" aria-labelledby={`${id}-title`}>
      <header><div><span className="eyebrow">Task coordination</span><h2 id={`${id}-title`}>Prerequisites</h2></div><button type="button" disabled={saving} onClick={requestClose}>Close</button></header>
      <form id={`${id}-form`} className="task-detail-content task-detail-editor" onSubmit={(event) => void submit(event)}>
        <section>
          <h3>{task.title}</h3>
          <p>Work that must finish before this task can resume. Queen usually manages these links; changing one does not start or stop a worker.</p>
          <label htmlFor={`${id}-operation`}>Change</label>
          <select id={`${id}-operation`} disabled={saving} value={operation} onChange={(event) => { setOperation(event.target.value as "add" | "remove"); setTarget(""); setQuery(""); setError(""); }}>
            <option value="add" disabled={!canAdd}>Add prerequisite</option>
            <option value="remove" disabled={existing.length === 0}>Remove prerequisite</option>
          </select>
          {!canAdd && <p>{task.state !== "blocked" ? "Only blocked tasks can gain a prerequisite. Record the actual block first; no task state is changed here." : "This task has reached its 32-prerequisite limit. Remove obsolete links first."}</p>}
          <label htmlFor={`${id}-query`}>Find task</label>
          <input id={`${id}-query`} disabled={saving} value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search by title or task ID" />
          <label htmlFor={`${id}-target`}>Prerequisite task</label>
          <select id={`${id}-target`} disabled={saving} value={target} onChange={(event) => { setTarget(event.target.value); setError(""); }}>
            <option value="">Choose a task</option>
            {visible.map((item) => <option key={item.id} value={item.id}>{item.title} · {item.state.replaceAll("_", " ")}</option>)}
          </select>
          {matching.length > 50 && <small>Showing the first 50 matches. Narrow your search to find more.</small>}
          {matching.length === 0 && <p>No matching tasks are available for this change.</p>}
          <label htmlFor={`${id}-reason`}>Why change this link?</label>
          <textarea id={`${id}-reason`} disabled={saving} value={reason} maxLength={2048} rows={3} onChange={(event) => setReason(event.target.value)} />
          {reasonTooLong && <p role="alert">Shorten the reason; it must fit within 2,048 bytes.</p>}
          <p>Your reason is recorded in task history. Only completed, non-removed work satisfies a prerequisite.</p>
          {error && <p role="alert" className="task-detail-save-error">{error}</p>}
        </section>
      </form>
      <footer>{confirmClose ? <UnsavedChangesPrompt label="Unsaved prerequisite change" description="Your current choices will be discarded." onDiscard={onClose} onKeep={() => setConfirmClose(false)} /> : <button form={`${id}-form`} disabled={saving || !valid}>{saving ? "Saving…" : operation === "add" ? "Add prerequisite" : "Remove prerequisite"}</button>}</footer>
    </section>
  </div>;
}
