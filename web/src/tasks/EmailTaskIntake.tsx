import { useEffect, useState } from "react";

import {
  fetchEmailInbox,
  fetchEmailAttachmentPreview,
  fetchEmailMessage,
  fetchEmailReadiness,
  importEmailMessage,
  type EmailMessage,
  type EmailMessageSummary,
  type EmailReadiness,
  type TaskPriority,
} from "../api";

type Props = {
  operatorToken: string;
  onImported: () => Promise<void>;
};

export default function EmailTaskIntake({ operatorToken, onImported }: Props) {
  const [readiness, setReadiness] = useState<EmailReadiness>();
  const [messages, setMessages] = useState<EmailMessageSummary[]>([]);
  const [selected, setSelected] = useState<EmailMessage>();
  const [query, setQuery] = useState("");
  const [priority, setPriority] = useState<TaskPriority>("normal");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [imageUrls, setImageUrls] = useState<Record<string, string>>({});

  useEffect(() => {
    let cancelled = false;
    void fetchEmailReadiness(operatorToken)
      .then((next) => { if (!cancelled) setReadiness(next); })
      .catch(() => { if (!cancelled) setReadiness(undefined); });
    return () => { cancelled = true; };
  }, [operatorToken]);

  useEffect(() => {
    if (readiness?.connection !== "ready" || selected) return;
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
  }, [operatorToken, query, readiness?.connection, selected]);

  useEffect(() => {
    const urls: string[] = [];
    setImageUrls({});
    if (!selected) return;
    let cancelled = false;
    const previews = selected.attachments.filter((attachment) =>
      attachment.inline && ["image/png", "image/jpeg", "image/gif", "image/webp"].includes(attachment.media_type),
    );
    void Promise.all(previews.map(async (attachment) => {
      const blob = await fetchEmailAttachmentPreview(operatorToken, selected.summary.id, attachment.id);
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
  }, [operatorToken, selected]);

  async function choose(summary: EmailMessageSummary) {
    setBusy(true);
    setMessage("");
    try {
      setSelected(await fetchEmailMessage(operatorToken, summary.id));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "That message could not be opened.");
    } finally {
      setBusy(false);
    }
  }

  async function importSelected() {
    if (!selected) return;
    setBusy(true);
    setMessage("");
    try {
      const imported = await importEmailMessage(operatorToken, selected.summary.id, priority);
      await onImported();
      setMessages((current) => current.filter((item) => item.id !== selected.summary.id));
      setSelected(undefined);
      setPriority("normal");
      setMessage(imported.created ? "Email added as a draft task." : "That email was already on the board; its existing task was kept.");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "That email could not be imported.");
    } finally {
      setBusy(false);
    }
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
        <div><p className="eyebrow">Email work</p><h3 id="email-work-heading">Choose a message from Inbox</h3></div>
        <span>{messages.length ? `${messages.length} recent messages` : "Inbox"} · {readiness.account_address}</span>
      </div>
      {!selected ? (
        <>
          <label className="jira-intake-filter">
            <span>Find a message</span>
            <input value={query} placeholder="Subject, sender, or message text" autoComplete="off" onChange={(event) => setQuery(event.target.value)} />
          </label>
          <div className="email-message-list" role="list" aria-label="Inbox messages">
            {messages.map((item) => (
              <button key={item.id} type="button" role="listitem" disabled={busy} onClick={() => void choose(item)}>
                <span className="email-message-sender">{item.sender_name || item.sender_address}</span>
                <span className="email-message-content"><strong>{item.subject || "(No subject)"}</strong><small>{item.preview || "No message preview"}</small></span>
                <time dateTime={new Date(item.received_at * 1000).toISOString()}>{formatReceived(item.received_at)}</time>
                {item.has_attachments ? <span className="email-attachment-mark">Attachments</span> : null}
              </button>
            ))}
            {!busy && messages.length === 0 ? <p className="jira-intake-empty">No Inbox messages match this view.</p> : null}
          </div>
        </>
      ) : (
        <article className="email-message-detail">
          <div className="email-detail-toolbar">
            <button className="text-button" type="button" onClick={() => { setSelected(undefined); setMessage(""); }}>← Inbox</button>
            <a href={selected.summary.web_url} target="_blank" rel="noreferrer">Open original</a>
          </div>
          <header>
            <h4>{selected.summary.subject || "(No subject)"}</h4>
            <p>{selected.summary.sender_name || selected.summary.sender_address} · {selected.summary.sender_address}</p>
            <small>{formatReceived(selected.summary.received_at)}</small>
          </header>
          <div className="email-body-preview">{readableBody(selected.body_text || selected.summary.preview || "No readable message body.")}</div>
          {Object.keys(imageUrls).length ? (
            <div className="email-inline-images" aria-label="Images in this message">
              {selected.attachments.filter((attachment) => imageUrls[attachment.id]).map((attachment) => (
                <figure key={attachment.id}>
                  <img src={imageUrls[attachment.id]} alt={attachment.name || "Image from email"} />
                  <figcaption>{attachment.name}</figcaption>
                </figure>
              ))}
            </div>
          ) : null}
          {selected.attachments.length ? (
            <div className="email-attachment-list">
              <strong>{selected.attachments.length} attachment{selected.attachments.length === 1 ? "" : "s"}</strong>
              {selected.attachments.map((attachment) => <span key={attachment.id}>{attachment.name} · {formatBytes(attachment.byte_size)}</span>)}
            </div>
          ) : null}
          <div className="email-import-actions">
            <label><span>Task priority</span><select value={priority} onChange={(event) => setPriority(event.target.value as TaskPriority)}><option value="low">Low</option><option value="normal">Normal</option><option value="high">High</option><option value="urgent">Urgent</option></select></label>
            <button className="primary-action" type="button" disabled={busy} onClick={() => void importSelected()}>{busy ? "Importing message…" : "Import as task"}</button>
          </div>
          <small className="privacy-note">Swarm stores a readable snapshot and private attachment copies. The original Outlook thread remains linked for the final reviewed reply.</small>
        </article>
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
