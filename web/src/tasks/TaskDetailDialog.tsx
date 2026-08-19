import { useEffect, useId, useRef, useState, type FormEvent } from "react";

import {
  fetchJiraTaskAttachment,
  fetchJiraTaskDetail,
  fetchEmailTaskAttachment,
  type EmailTaskSource,
  type JiraTaskAttachment,
  type JiraTaskLink,
  type Task,
  type TaskPriority,
  type TaskUpdateInput,
} from "../api";
import { useModalFocus } from "../shared/useModalFocus";
import UnsavedChangesPrompt from "../shared/UnsavedChangesPrompt";

type LoadedImage = JiraTaskAttachment & { url: string };
const noEmailSources: EmailTaskSource[] = [];

export default function TaskDetailDialog({ task, jiraLink, emailSources = noEmailSources, operatorToken, busy, onClose, onSave, onRemove }: {
  task: Task;
  jiraLink?: JiraTaskLink;
  emailSources?: EmailTaskSource[];
  operatorToken: string;
  busy: boolean;
  onClose: () => void;
  onSave: (input: TaskUpdateInput) => Promise<void>;
  onRemove: () => Promise<void>;
}) {
  const titleId = useId();
  const formId = useId();
  const [title, setTitle] = useState(task.title);
  const [description, setDescription] = useState(task.description);
  const [instruction, setInstruction] = useState(task.operator_instruction ?? "");
  const [sourceDescription, setSourceDescription] = useState("");
  const [priority, setPriority] = useState(task.priority);
  const [attachments, setAttachments] = useState<JiraTaskAttachment[]>([]);
  const [images, setImages] = useState<LoadedImage[]>([]);
  const [imageLoadFailures, setImageLoadFailures] = useState(0);
  const [loading, setLoading] = useState(Boolean(jiraLink || emailSources.length));
  const [failed, setFailed] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const [removeConfirm, setRemoveConfirm] = useState(false);
  const [removeFailed, setRemoveFailed] = useState(false);
  const [closeConfirm, setCloseConfirm] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const titleInput = useRef<HTMLInputElement>(null);
  const keepTaskButton = useRef<HTMLButtonElement>(null);
  const dirty = title !== task.title
    || description !== task.description
    || instruction !== (task.operator_instruction ?? "")
    || priority !== task.priority;
  function requestClose() {
    if (removeConfirm) return setRemoveConfirm(false);
    if (closeConfirm) return setCloseConfirm(false);
    if (dirty) return setCloseConfirm(true);
    onClose();
  }
  const dialog = useModalFocus<HTMLElement>(requestClose, true, titleInput);

  useEffect(() => {
    if (removeConfirm) keepTaskButton.current?.focus();
  }, [removeConfirm]);

  useEffect(() => {
    let active = true;
    const objectUrls: string[] = [];
    setLoading(true);
    setFailed(false);
    setImageLoadFailures(0);
    void (async () => {
      try {
        const emailAttachments: JiraTaskAttachment[] = emailSources.flatMap((source) => source.attachments.map((attachment) => ({
          id: `email:${source.id}:${attachment.storage_name}`,
          filename: attachment.display_name,
          media_type: attachment.media_type,
          byte_size: attachment.byte_size,
          is_image: attachment.media_type.startsWith("image/"),
        })));
        const detail = jiraLink ? await fetchJiraTaskDetail(operatorToken, task.id) : undefined;
        if (!active) return;
        setSourceDescription(detail?.description || "");
        const allAttachments = [...(detail?.attachments ?? []), ...emailAttachments];
        setAttachments(allAttachments);
        let failedImages = 0;
        const loaded = (await Promise.all(allAttachments.filter((attachment) => attachment.is_image).map(async (attachment) => {
          try {
            const blob = attachment.id.startsWith("email:")
              ? await fetchEmailTaskAttachment(operatorToken, task.id, attachment.id.split(":").slice(2).join(":"))
              : await fetchJiraTaskAttachment(operatorToken, task.id, attachment.id);
            if (!active) return null;
            const url = URL.createObjectURL(blob);
            objectUrls.push(url);
            return { ...attachment, url };
          } catch {
            failedImages += 1;
            return null;
          }
        }))).filter((image): image is LoadedImage => image !== null);
        if (active) {
          setImages(loaded);
          setImageLoadFailures(failedImages);
        }
      } catch {
        if (active) setFailed(true);
      } finally {
        if (active) setLoading(false);
      }
    })();
    return () => {
      active = false;
      objectUrls.forEach((url) => URL.revokeObjectURL(url));
    };
  }, [attempt, emailSources, jiraLink, operatorToken, task.description, task.id]);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!title.trim()) return;
    setSaveFailed(false);
    try {
      await onSave({ title, description, priority, operator_instruction: instruction });
      onClose();
    } catch {
      setSaveFailed(true);
    }
  }

  async function remove() {
    setRemoveFailed(false);
    try {
      await onRemove();
      onClose();
    } catch {
      setRemoveFailed(true);
    }
  }

  return (
    <div className="task-detail-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) requestClose(); }}>
      <section ref={dialog} tabIndex={-1} className="task-detail-dialog" role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <header>
          <div>
            <span className="eyebrow">Task details</span>
            <h2 id={titleId}>Review and edit task</h2>
          </div>
          <button type="button" onClick={requestClose}>Close</button>
        </header>
        <div className="task-detail-summary" aria-label="Task summary">
          <span><small>Swarm status</small><strong>{task.state}</strong></span>
          <span><small>Priority</small><strong>{task.priority}</strong></span>
          {jiraLink && <span><small>Jira issue</small><strong>{jiraLink.issue_key}</strong></span>}
          {jiraLink && <span><small>Jira status</small><strong>{jiraLink.jira_status_name}</strong></span>}
          {jiraLink && <span><small>Project</small><strong>{jiraLink.project_name}</strong></span>}
          {jiraLink && <span><small>Jira assignee</small><strong>{jiraLink.jira_assignee_name || "Unassigned"}</strong></span>}
        </div>
        <form id={formId} className="task-detail-content task-detail-editor" onSubmit={(event) => void submit(event)}>
          <section>
            <label htmlFor={`detail-title-${task.id}`}>Title</label>
            <input ref={titleInput} id={`detail-title-${task.id}`} value={title} onChange={(event) => setTitle(event.target.value)} maxLength={240} />
            <label htmlFor={`detail-description-${task.id}`}>Work brief</label>
            <textarea id={`detail-description-${task.id}`} value={description} onChange={(event) => setDescription(event.target.value)} maxLength={10000} rows={6} placeholder="Add the outcome, context, and what done looks like" />
            <label htmlFor={`detail-instruction-${task.id}`}>How to approach this</label>
            <input
              id={`detail-instruction-${task.id}`}
              value={instruction}
              onChange={(event) => setInstruction(event.target.value)}
              maxLength={280}
              placeholder="Interview me first · Analyse this, do not act on it"
              aria-describedby={`detail-instruction-help-${task.id}`}
            />
            <small id={`detail-instruction-help-${task.id}`} className="field-help">One line, kept apart from the brief and read out to the worker before the work.</small>
            <label htmlFor={`detail-priority-${task.id}`}>Priority</label>
            <select id={`detail-priority-${task.id}`} value={priority} onChange={(event) => setPriority(event.target.value as TaskPriority)}>
              <option value="low">Low</option><option value="normal">Normal</option><option value="high">High</option><option value="urgent">Urgent</option>
            </select>
          </section>
          {sourceDescription && sourceDescription !== description && <section><h3>Source description</h3><p className="task-detail-description">{sourceDescription}</p></section>}
          {loading && <p className="task-detail-loading">Loading task details and images…</p>}
          {failed && <div className="task-detail-error"><span>Some linked details could not be loaded. The saved Swarm description is still shown.</span><button type="button" onClick={() => setAttempt((value) => value + 1)}>Try again</button></div>}
          {!failed && imageLoadFailures > 0 && <div className="task-detail-error" role="alert"><span>{imageLoadFailures} linked image{imageLoadFailures === 1 ? "" : "s"} could not be loaded. Attachment names and the rest of this task are still available.</span><button type="button" onClick={() => setAttempt((value) => value + 1)}>Retry images</button></div>}
          {images.length > 0 && (
            <section>
              <h3>Images</h3>
              <div className="task-detail-gallery">
                {images.map((image) => <figure key={image.id}><img src={image.url} alt={image.filename} /><figcaption>{image.filename}</figcaption></figure>)}
              </div>
            </section>
          )}
          {attachments.length > 0 && (
            <section>
              <h3>Attachments <span>{attachments.length}</span></h3>
              <ul className="task-detail-attachments" aria-label="Attachments">
                {attachments.map((attachment) => <li key={attachment.id}><span>{attachment.filename}</span><small>{formatBytes(attachment.byte_size)} · {attachment.media_type}</small></li>)}
              </ul>
            </section>
          )}
          {saveFailed && <p className="task-detail-save-error" role="alert">Changes were not saved. Your edits are still here—try again when the connection is ready.</p>}
        </form>
        <footer>
          {closeConfirm ? <UnsavedChangesPrompt label="Unsaved task changes" description="Your edits have not joined the task history." onDiscard={onClose} onKeep={() => setCloseConfirm(false)} /> : <><div className="task-detail-footer-secondary">
            {jiraLink?.issue_url && <a className="button-link" href={jiraLink.issue_url} target="_blank" rel="noreferrer">Open in Jira</a>}
            {!removeConfirm ? (
              <button type="button" className="secondary-button danger-text" disabled={busy || task.state === "active" || task.state === "review"} onClick={() => setRemoveConfirm(true)}>Remove from Hive</button>
            ) : (
              <div className="task-remove-confirm" role="alertdialog" aria-label="Confirm task removal">
                <p><strong>{jiraLink ? `Remove ${jiraLink.issue_key} from Swarm?` : "Remove this task from the Hive?"}</strong><span>{jiraLink ? "The Jira issue will not be deleted or changed. Its source link and Swarm audit history stay retained." : "The task leaves the board, but its source, attachments, and audit history stay retained."}</span></p>
                <button type="button" className="danger-button" disabled={busy} onClick={() => void remove()}>{busy ? "Removing…" : "Remove from Hive"}</button>
                <button ref={keepTaskButton} type="button" disabled={busy} onClick={() => setRemoveConfirm(false)}>Keep task</button>
              </div>
            )}
            {(task.state === "active" || task.state === "review") && <small>Finish or move this work out of progress before removing it.</small>}
            {removeFailed && <small className="task-detail-save-error" role="alert">The task was not removed. Nothing changed—try again when the connection is ready.</small>}
          </div>
          <button form={formId} disabled={busy || !title.trim()}>{busy ? "Saving…" : "Save changes"}</button></>}
        </footer>
      </section>
    </div>
  );
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}
