import { useState } from "react";

import type { UnansweredEmailTask } from "../api";
import BeeMascot from "../brand/BeeMascot";

/**
 * Completed email tasks whose requester has not been answered — ONE CARD EACH,
 * read and edited in place.
 *
 * A task imported from email carries a person waiting on it, and finishing the
 * work tells them nothing: the reply is a separate, deliberate step. Nothing
 * used to notice when that step never happened, so a worker could close the
 * task and the person who wrote in heard nothing at all.
 *
 * ONE CARD EACH is the correction that matters. This used to render
 * `const [first] = awaiting` and reduce every other waiting person to the
 * phrase "and 2 others like it" — unreadable, unsendable, unreachable. The only
 * route to the second was to send the first, wait for the list to change, and
 * look again. The operator asked for exactly this: "separate each email into a
 * separate for you item that I could quickly scan through and approve or edit
 * for sending."
 *
 * THE DRAFT IS SHOWN WHOLE, and that is a correction of a correction. It was
 * briefly truncated to 45 words, reasoning that a wall of text buries the
 * people below it. That reasoning was already spent: the per-card split is what
 * stopped anyone being buried, so cutting sentences bought nothing and cost
 * sense. The operator: "we jumped to the other ditch. The view of the reply gets
 * cut off with an ellipse and doesn't make sense." The block scrolls instead,
 * and the word count stays, because "how long is this" is the complaint that
 * started all of it.
 *
 * EDITING HAPPENS HERE. It used to throw the operator to the task board to go
 * and find the task the reply belonged to — "this should stay on the task
 * page". Reading and fixing the words is the only part of an email task that is
 * genuinely theirs, and it was the one part that sent them somewhere else.
 */
export default function UnansweredEmailAttentionCard({ awaiting, busy, onOpenTask, onSendReply, onSaveReply }: {
  awaiting: UnansweredEmailTask[];
  busy?: boolean;
  onOpenTask: (taskId: string) => void;
  onSendReply?: (replyId: string) => void;
  onSaveReply?: (taskId: string, body: string) => void;
}) {
  if (awaiting.length === 0) return null;
  return (
    <>
      {awaiting.map((item) => (
        <UnansweredEmailCard
          key={item.task_id}
          item={item}
          busy={busy}
          onOpenTask={onOpenTask}
          onSendReply={onSendReply}
          onSaveReply={onSaveReply}
        />
      ))}
    </>
  );
}

function UnansweredEmailCard({ item, busy, onOpenTask, onSendReply, onSaveReply }: {
  item: UnansweredEmailTask;
  busy?: boolean;
  onOpenTask: (taskId: string) => void;
  onSendReply?: (replyId: string) => void;
  onSaveReply?: (taskId: string, body: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [body, setBody] = useState(item.draft_body ?? "");
  const heading = `unanswered-email-${item.task_id}`;
  const canEditHere = Boolean(item.draft_body && onSaveReply);
  return (
    <section className="queen-attention-card" aria-labelledby={heading}>
      <span className="queen-attention-bee" aria-hidden="true"><BeeMascot expression="focused" /></span>
      <div>
        {/* Whose work this was. The queue said something needed the operator
            and never said who it belonged to, so every card looked the same. */}
        <p className="eyebrow">{item.worker_name ? `${item.worker_name} · Email` : "Email"}</p>
        <h3 id={heading}>{recipientSummary(item)}</h3>
        <p>
          “{item.title}”.
          {/* How many people one press of Send reaches. Naming only the earliest
              sender made a seven-thread send look identical to a one-thread
              send, and this Hive has sent to seven. */}
          {item.thread_count > 1 ? ` Sending answers all ${item.thread_count} original threads at once.` : ""}
          {item.draft_body ? " A reply is written and waiting for you to read it." : item.drafted ? " A reply is written but was never sent." : " No reply has been written."}
        </p>
        {editing ? (
          <label className="unanswered-email-editor">
            <span className="field-caption">Reply to {item.sender_name || item.sender_address}</span>
            <textarea rows={12} value={body} disabled={busy} onChange={(event) => setBody(event.target.value)} />
            <small>{countWords(body)} words</small>
          </label>
        ) : item.draft_body ? (
          <blockquote className="unanswered-email-draft">
            {item.draft_body}
            <small>{countWords(item.draft_body)} words</small>
          </blockquote>
        ) : null}
      </div>
      <div className="queen-attention-actions">
        {editing ? (
          <>
            <button
              className="primary-action"
              type="button"
              disabled={busy || !body.trim() || body.trim() === item.draft_body}
              onClick={() => { onSaveReply?.(item.task_id, body.trim()); setEditing(false); }}
            >Save changes</button>
            <button
              className="secondary-button"
              type="button"
              disabled={busy}
              onClick={() => { setBody(item.draft_body ?? ""); setEditing(false); }}
            >Cancel</button>
          </>
        ) : (
          <>
            {item.draft_id && onSendReply ? (
              <button className="primary-action" type="button" disabled={busy} onClick={() => onSendReply(item.draft_id!)}>
                Send this reply
              </button>
            ) : null}
            {canEditHere ? (
              <button className="secondary-button" type="button" disabled={busy} onClick={() => { setBody(item.draft_body ?? ""); setEditing(true); }}>
                Edit here
              </button>
            ) : null}
            {/* Still a route to the task itself, for everything a reply editor
                is not — the original thread, its attachments, the history. It
                is no longer the only way to change a word. */}
            <button className={item.draft_id ? "text-button" : "primary-action"} type="button" onClick={() => onOpenTask(item.task_id)}>
              Open the task
            </button>
          </>
        )}
      </div>
    </section>
  );
}

/**
 * Who this one send actually reaches, in the heading.
 *
 * The plural is not cosmetic: it is the difference between answering a person
 * and answering a room, and the operator decides whether to press Send from
 * this line.
 */
function recipientSummary(item: UnansweredEmailTask): string {
  const who = item.sender_name || item.sender_address;
  if (item.thread_count <= 1) return `${who} is waiting on a reply`;
  const others = item.thread_count - 1;
  return `${who} and ${others} other${others === 1 ? "" : "s"} are waiting on a reply`;
}

/** Length in words, which is how a reader judges "is this too long to send". */
function countWords(body: string): number {
  return body.trim().split(/\s+/).filter(Boolean).length;
}
