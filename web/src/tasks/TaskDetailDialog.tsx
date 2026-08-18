import { useEffect, useId, useState } from "react";

import {
  fetchJiraTaskAttachment,
  fetchJiraTaskDetail,
  fetchEmailTaskAttachment,
  type EmailTaskSource,
  type JiraTaskAttachment,
  type JiraTaskLink,
  type Task,
} from "../api";

type LoadedImage = JiraTaskAttachment & { url: string };

export default function TaskDetailDialog({ task, jiraLink, emailSources = [], operatorToken, onClose, onEdit }: {
  task: Task;
  jiraLink?: JiraTaskLink;
  emailSources?: EmailTaskSource[];
  operatorToken: string;
  onClose: () => void;
  onEdit: () => void;
}) {
  const titleId = useId();
  const [description, setDescription] = useState(task.description);
  const [attachments, setAttachments] = useState<JiraTaskAttachment[]>([]);
  const [images, setImages] = useState<LoadedImage[]>([]);
  const [loading, setLoading] = useState(Boolean(jiraLink || emailSources.length));
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  useEffect(() => {
    let active = true;
    const objectUrls: string[] = [];
    setLoading(true);
    setFailed(false);
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
        setDescription(detail?.description || task.description);
        const allAttachments = [...(detail?.attachments ?? []), ...emailAttachments];
        setAttachments(allAttachments);
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
            return null;
          }
        }))).filter((image): image is LoadedImage => image !== null);
        if (active) setImages(loaded);
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

  return (
    <div className="task-detail-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <section className="task-detail-dialog" role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <header>
          <div>
            <span className="eyebrow">Task details</span>
            <h2 id={titleId}>{task.title}</h2>
          </div>
          <button type="button" autoFocus onClick={onClose}>Close</button>
        </header>
        <div className="task-detail-summary" aria-label="Task summary">
          <span><small>Swarm status</small><strong>{task.state}</strong></span>
          <span><small>Priority</small><strong>{task.priority}</strong></span>
          {jiraLink && <span><small>Jira issue</small><strong>{jiraLink.issue_key}</strong></span>}
          {jiraLink && <span><small>Jira status</small><strong>{jiraLink.jira_status_name}</strong></span>}
          {jiraLink && <span><small>Project</small><strong>{jiraLink.project_name}</strong></span>}
          {jiraLink && <span><small>Jira assignee</small><strong>{jiraLink.jira_assignee_name || "Unassigned"}</strong></span>}
        </div>
        <div className="task-detail-content">
          <section>
            <h3>Description</h3>
            {description ? <p className="task-detail-description">{description}</p> : <p className="empty-note">No description was provided.</p>}
          </section>
          {loading && <p className="task-detail-loading">Loading task details and images…</p>}
          {failed && <div className="task-detail-error"><span>Some linked details could not be loaded. The saved Swarm description is still shown.</span><button type="button" onClick={() => setAttempt((value) => value + 1)}>Try again</button></div>}
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
              <ul className="task-detail-attachments">
                {attachments.map((attachment) => <li key={attachment.id}><span>{attachment.filename}</span><small>{formatBytes(attachment.byte_size)} · {attachment.media_type}</small></li>)}
              </ul>
            </section>
          )}
        </div>
        <footer>
          {jiraLink?.issue_url && <a className="button-link" href={jiraLink.issue_url} target="_blank" rel="noreferrer">Open in Jira</a>}
          <button type="button" onClick={onEdit}>Edit Swarm task</button>
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
