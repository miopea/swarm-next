import { useEffect, useMemo, useState } from "react";

import {
  fetchEmailInbox,
  fetchEmailAttachmentPreview,
  fetchEmailMessage,
  fetchEmailReadiness,
  importEmailTask,
  type EmailMessage,
  type EmailMessageSummary,
  type EmailReadiness,
  type TaskPriority,
  type Worker,
} from "../api";

type Props = {
  operatorToken: string;
  workers?: Worker[];
  onImported: () => Promise<void>;
};

export default function EmailTaskIntake({ operatorToken, workers = [], onImported }: Props) {
  const [readiness, setReadiness] = useState<EmailReadiness>();
  const [messages, setMessages] = useState<EmailMessageSummary[]>([]);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [selectedMessages, setSelectedMessages] = useState<EmailMessage[]>([]);
  const [previewIndex, setPreviewIndex] = useState(0);
  const [query, setQuery] = useState("");
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [priority, setPriority] = useState<TaskPriority>("normal");
  const [initialState, setInitialState] = useState<"draft" | "ready">("draft");
  const [workerId, setWorkerId] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [imageUrls, setImageUrls] = useState<Record<string, string>>({});
  const assignableWorkers = useMemo(() => workers.filter((worker) => worker.role !== "queen"), [workers]);
  const preview = selectedMessages[previewIndex];

  useEffect(() => {
    let cancelled = false;
    void fetchEmailReadiness(operatorToken)
      .then((next) => { if (!cancelled) setReadiness(next); })
      .catch(() => { if (!cancelled) setReadiness(undefined); });
    return () => { cancelled = true; };
  }, [operatorToken]);

  useEffect(() => {
    if (readiness?.connection !== "ready" || selectedMessages.length) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      setBusy(true);
      setMessage("");
      void fetchEmailInbox(operatorToken, query)
        .then((next) => {
          if (!cancelled) {
            setMessages(next);
            setMessage(next.length ? "Newest Inbox messages are shown first." : "No Inbox messages match this search.");
          }
        })
        .catch((error: unknown) => {
          if (!cancelled) setMessage(error instanceof Error ? error.message : "Inbox could not be loaded.");
        })
        .finally(() => { if (!cancelled) setBusy(false); });
    }, query.trim() ? 250 : 0);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [operatorToken, query, readiness?.connection, selectedMessages.length]);

  useEffect(() => {
    const urls: string[] = [];
    setImageUrls({});
    if (!preview) return;
    let cancelled = false;
    const previews = preview.attachments.filter((attachment) =>
      ["image/png", "image/jpeg", "image/gif", "image/webp"].includes(attachment.media_type),
    );
    void Promise.all(previews.map(async (attachment) => {
      const blob = await fetchEmailAttachmentPreview(operatorToken, preview.summary.id, attachment.id);
      const url = URL.createObjectURL(blob);
      urls.push(url);
      return [attachment.id, url] as const;
    })).then((entries) => {
      if (!cancelled) setImageUrls(Object.fromEntries(entries));
    }).catch(() => {
      if (!cancelled) setMessage("One or more inline images could not be previewed. They will still be preserved on import.");
    });
    return () => {
      cancelled = true;
      urls.forEach((url) => URL.revokeObjectURL(url));
    };
  }, [operatorToken, preview]);

  function toggle(messageId: string) {
    setSelectedIds((current) => current.includes(messageId)
      ? current.filter((id) => id !== messageId)
      : current.length < 20 ? [...current, messageId] : current);
  }

  async function reviewSelection() {
    if (!selectedIds.length) return;
    setBusy(true);
    setMessage("");
    try {
      const selected = await Promise.all(selectedIds.map((id) => fetchEmailMessage(operatorToken, id)));
      setSelectedMessages(selected);
      setPreviewIndex(0);
      setTitle(selected.length === 1
        ? selected[0].summary.subject || "Email request"
        : `${selected[0].summary.subject || "Related email requests"} (+${selected.length - 1} related)`);
      setDescription(selected.map((item) => [
        `From: ${item.summary.sender_name || item.summary.sender_address} <${item.summary.sender_address}>`,
        `Received: ${formatReceived(item.summary.received_at)}`,
        item.body_text || item.summary.preview || "No readable message body.",
      ].join("\n")).join("\n\n--- Related email ---\n\n"));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "The selected messages could not be opened.");
    } finally {
      setBusy(false);
    }
  }

  async function importSelected() {
    if (!selectedMessages.length || !title.trim()) return;
    setBusy(true);
    setMessage("");
    try {
      const imported = await importEmailTask(operatorToken, {
        message_ids: selectedMessages.map((item) => item.summary.id),
        title: title.trim(),
        description: description.trim(),
        priority,
        worker_id: workerId || null,
        state: initialState,
      });
      await onImported();
      const importedIds = new Set(selectedMessages.map((item) => item.summary.id));
      setMessages((current) => current.filter((item) => !importedIds.has(item.id)));
      setSelectedIds([]);
      setSelectedMessages([]);
      setPreviewIndex(0);
      setTitle("");
      setDescription("");
      setPriority("normal");
      setInitialState("draft");
      setWorkerId("");
      setMessage(imported.created
        ? `${imported.sources.length} email${imported.sources.length === 1 ? "" : "s"} added as one task.`
        : "Those emails were already on the board; their existing task was kept.");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Those emails could not be imported.");
    } finally {
      setBusy(false);
    }
  }

  function backToInbox() {
    setSelectedMessages([]);
    setPreviewIndex(0);
    setImageUrls({});
    setMessage("");
  }

  if (readiness?.connection !== "ready") {
    return (
      <section className="email-task-source email-not-connected" aria-labelledby="email-work-heading">
        <div><p className="eyebrow">Email work</p><h3 id="email-work-heading">Connect Outlook first</h3></div>
        <p>Link the one issue-intake account in Settings → Integrations, then return here to choose Inbox messages.</p>
      </section>
    );
  }

  return (
    <section className="email-task-source" aria-labelledby="email-work-heading">
      <div className="email-intake-heading">
        <div><p className="eyebrow">Email work</p><h3 id="email-work-heading">{selectedMessages.length ? "Review the task before import" : "Choose messages from Inbox"}</h3></div>
        <span>{selectedMessages.length ? `${selectedMessages.length} source thread${selectedMessages.length === 1 ? "" : "s"}` : `${messages.length || "Inbox"} · ${readiness.account_address}`}</span>
      </div>
      {!selectedMessages.length ? (
        <>
          <label className="jira-intake-filter">
            <span>Find a message</span>
            <input value={query} placeholder="Subject, sender, or message text" autoComplete="off" onChange={(event) => setQuery(event.target.value)} />
          </label>
          <div className="email-message-list email-message-selection" role="list" aria-label="Inbox messages">
            {messages.map((item) => (
              <label key={item.id} role="listitem" className={selectedIds.includes(item.id) ? "selected" : ""}>
                <input type="checkbox" checked={selectedIds.includes(item.id)} disabled={busy} onChange={() => toggle(item.id)} />
                <span className="email-message-sender">{item.sender_name || item.sender_address}</span>
                <span className="email-message-content"><strong>{item.subject || "(No subject)"}</strong><small>{item.preview || "No message preview"}</small></span>
                <time dateTime={new Date(item.received_at * 1000).toISOString()}>{formatReceived(item.received_at)}</time>
                {item.has_attachments ? <span className="email-attachment-mark">Attachments</span> : null}
              </label>
            ))}
            {!busy && messages.length === 0 ? <p className="jira-intake-empty">No Inbox messages match this view.</p> : null}
          </div>
          <div className="email-selection-actions">
            <span>{selectedIds.length ? `${selectedIds.length} selected` : "Select one message, or combine related reports."}</span>
            <button className="primary-action" type="button" disabled={busy || !selectedIds.length} onClick={() => void reviewSelection()}>{busy ? "Opening…" : `Review ${selectedIds.length || ""} message${selectedIds.length === 1 ? "" : "s"}`}</button>
          </div>
        </>
      ) : (
        <div className="email-import-review">
          <section className="email-task-setup" aria-labelledby="email-task-setup-heading">
            <div><p className="eyebrow">Task setup</p><h4 id="email-task-setup-heading">Shape the work before it joins the board</h4></div>
            <div className="email-task-fields">
              <div className="email-task-field email-task-title"><label htmlFor="email-task-title">Task title</label><input id="email-task-title" value={title} maxLength={240} onChange={(event) => setTitle(event.target.value)} /></div>
              <div className="email-task-field email-task-description"><label htmlFor="email-task-description">Task description</label><textarea id="email-task-description" value={description} rows={6} onChange={(event) => setDescription(event.target.value)} /></div>
              <div className="email-task-field"><label htmlFor="email-task-priority">Priority</label><select id="email-task-priority" value={priority} onChange={(event) => setPriority(event.target.value as TaskPriority)}><option value="low">Low</option><option value="normal">Normal</option><option value="high">High</option><option value="urgent">Urgent</option></select></div>
              <div className="email-task-field"><label htmlFor="email-task-status">Starting status</label><select id="email-task-status" value={initialState} onChange={(event) => setInitialState(event.target.value as "draft" | "ready")}><option value="draft">Draft — review later</option><option value="ready">Ready — available to work</option></select></div>
              <div className="email-task-field email-task-worker"><label htmlFor="email-task-worker">Worker</label><select id="email-task-worker" value={workerId} onChange={(event) => setWorkerId(event.target.value)}><option value="">Unassigned</option>{assignableWorkers.map((worker) => <option key={worker.id} value={worker.id}>{worker.name} · {worker.attention_state}</option>)}</select></div>
            </div>
            <div className="email-import-actions"><small>Every original thread and attachment stays linked for a reviewed plain-language reply after completion and deployment.</small><button className="primary-action" type="button" disabled={busy || !title.trim()} onClick={() => void importSelected()}>{busy ? "Importing…" : `Import ${selectedMessages.length} email${selectedMessages.length === 1 ? "" : "s"} as one task`}</button></div>
          </section>
          <section className="email-source-workbench" aria-labelledby="email-source-review-heading">
            <div className="email-detail-toolbar">
              <button className="text-button" type="button" onClick={backToInbox}>← Inbox</button>
              <span id="email-source-review-heading">Review {selectedMessages.length} source email{selectedMessages.length === 1 ? "" : "s"}</span>
            </div>
            <label className="email-source-select">
              <span>Preview source email</span>
              <select value={previewIndex} onChange={(event) => setPreviewIndex(Number(event.target.value))}>
                {selectedMessages.map((item, index) => <option key={item.summary.id} value={index}>{index + 1}. {item.summary.subject || "(No subject)"} — {item.summary.sender_name || item.summary.sender_address}</option>)}
              </select>
            </label>
            <div className="email-source-tabs" role="tablist" aria-label="Selected source emails">
              {selectedMessages.map((item, index) => <button key={item.summary.id} role="tab" aria-selected={index === previewIndex} onClick={() => setPreviewIndex(index)}><strong>{index + 1}. {item.summary.subject || "(No subject)"}</strong><small>{item.summary.sender_name || item.summary.sender_address}</small></button>)}
            </div>
            {preview ? <article className="email-message-detail email-source-preview">
              <div className="email-detail-toolbar"><span>{formatReceived(preview.summary.received_at)}</span><a href={preview.summary.web_url} target="_blank" rel="noreferrer">Open original</a></div>
              <header><h4>{preview.summary.subject || "(No subject)"}</h4><p>{preview.summary.sender_name || preview.summary.sender_address} · {preview.summary.sender_address}</p></header>
              <div className="email-body-preview">{readableBody(preview.body_text || preview.summary.preview || "No readable message body.")}</div>
              {Object.keys(imageUrls).length ? <div className="email-inline-images" aria-label="Images in this message">{preview.attachments.filter((attachment) => imageUrls[attachment.id]).map((attachment) => <figure key={attachment.id}><img src={imageUrls[attachment.id]} alt={attachment.name || "Image from email"} /><figcaption>{attachment.name}</figcaption></figure>)}</div> : null}
              {preview.attachments.length ? <div className="email-attachment-list"><strong>{preview.attachments.length} attachment{preview.attachments.length === 1 ? "" : "s"}</strong>{preview.attachments.map((attachment) => <span key={attachment.id}>{attachment.name} · {formatBytes(attachment.byte_size)}</span>)}</div> : null}
            </article> : null}
          </section>
        </div>
      )}
      {message ? <p className="settings-message" role="status">{message}</p> : null}
    </section>
  );
}

function formatReceived(timestamp: number) {
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(timestamp * 1000));
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function readableBody(body: string) {
  const withoutCidMarkers = body.replace(/^\s*\[cid:[^\]]+\]\s*$/gim, "").trim();
  return withoutCidMarkers.split(/\n{2,}/).filter(Boolean).map((paragraph, index) => <p key={`${index}-${paragraph.slice(0, 24)}`}>{paragraph}</p>);
}
