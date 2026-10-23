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
/** What saving as an Outlook draft came to, so the card can offer a reconnect. */
export type OutlookDraftResult = "saved" | "reconnect" | "failed";

/**
 * NOT EVERY EMAIL NEEDS AN ANSWER, AND NOT EVERY ANSWER SHOULD LEAVE FROM HERE
 * (ADR 0121). The operator: "Sometimes we don't need to send an email. So we
 * should have a way to not send an email, and also an option to save it as a
 * draft so it sits in my outlook." Send was the only way out of this card, so
 * a thread that needed no reply stayed in Needs you for good. "Don't send"
 * closes it with nothing sent; "Save as draft in Outlook" puts the reply in
 * their Drafts folder to finish there. Both leave the card, and both are
 * recorded on the task.
 */
type ClosingHandlers = {
  onRetryReply?: (replyId: string) => void;
  onDontSend?: (taskId: string) => void;
  onSaveAsDraft?: (taskId: string) => Promise<OutlookDraftResult>;
  onReconnectForDrafts?: () => void;
};

export default function UnansweredEmailAttentionCard({ awaiting, busy, onOpenTask, onSendReply, onSaveReply, onReviseReply, onRetryReply, onDontSend, onSaveAsDraft, onReconnectForDrafts }: {
  awaiting: UnansweredEmailTask[];
  busy?: boolean;
  onOpenTask: (taskId: string) => void;
  onSendReply?: (replyId: string) => void;
  onSaveReply?: (taskId: string, body: string) => void;
  onReviseReply?: (taskId: string, instruction: string) => Promise<string | null>;
} & ClosingHandlers) {
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
          onReviseReply={onReviseReply}
          onRetryReply={onRetryReply}
          onDontSend={onDontSend}
          onSaveAsDraft={onSaveAsDraft}
          onReconnectForDrafts={onReconnectForDrafts}
        />
      ))}
    </>
  );
}

function UnansweredEmailCard({ item, busy, onOpenTask, onSendReply, onSaveReply, onReviseReply, onRetryReply, onDontSend, onSaveAsDraft, onReconnectForDrafts }: {
  item: UnansweredEmailTask;
  busy?: boolean;
  onOpenTask: (taskId: string) => void;
  onSendReply?: (replyId: string) => void;
  onSaveReply?: (taskId: string, body: string) => void;
  onReviseReply?: (taskId: string, instruction: string) => Promise<string | null>;
} & ClosingHandlers) {
  // "Don't send" sits beside Send, so it asks once before it closes the card:
  // the item does not come back, and the person who wrote in hears nothing.
  const [confirmingDontSend, setConfirmingDontSend] = useState(false);
  const [needsDraftPermission, setNeedsDraftPermission] = useState(false);
  async function saveAsDraft() {
    if (!onSaveAsDraft) return;
    setNeedsDraftPermission((await onSaveAsDraft(item.task_id)) === "reconnect");
  }
  const [editing, setEditing] = useState(false);
  const [body, setBody] = useState(item.draft_body ?? "");
  const [instruction, setInstruction] = useState("");
  // What the last AI revision replaced. Undo is the whole safety of this
  // feature: the operator has already said of one draft "I like how it's
  // written", so the expensive failure is a prompt that overshoots and takes a
  // good draft with it. Held here rather than saved, so nothing is written
  // until they choose to write it.
  const [undoBody, setUndoBody] = useState<string | null>(null);
  const heading = `unanswered-email-${item.task_id}`;
  // Writing the FIRST reply happens here too. Every task on this queue is
  // Completed, and completing requires settled evidence, so a reply can always
  // be written from here — there is never a reason to send the operator to the
  // task page's deployment form. That form asked them to record something the
  // worker verifies, and for work closed on an approved exemption it asked for
  // a deployment that does not exist.
  const canEditHere = Boolean(onSaveReply);
  // ⚠️ A RESULT NOBODY KNOWS IS NOT A REPLY NOBODY WROTE. Microsoft may already
  // have delivered it, so it is not editable, closable or draftable here: each
  // could answer the person twice. The one action is a deliberate retry.
  const uncertain = Boolean(item.uncertain_reply_id);
  const settled = !item.sending && !uncertain;

  async function revise() {
    if (!onReviseReply || !instruction.trim()) return;
    const previous = body;
    const revised = await onReviseReply(item.task_id, instruction.trim());
    // A failed revision leaves the draft exactly as it was, and offers no undo
    // for a change that never happened.
    if (revised === null) return;
    setUndoBody(previous);
    setBody(revised);
    setInstruction("");
  }
  return (
    <section className="queen-attention-card" aria-labelledby={heading}>
      <span className="queen-attention-bee" aria-hidden="true"><BeeMascot expression="focused" /></span>
      <div>
        {/* Whose work this was. The queue said something needed the operator
            and never said who it belonged to, so every card looked the same. */}
        <p className="eyebrow">{item.worker_name ? `${item.worker_name} · Email` : "Email"}</p>
        <h3 id={heading}>{recipientSummary(item)}</h3>
        {/* A SEND THAT NEVER LEFT THE BUILDING. Seventeen replies were
            cancelled on 2026-08-25 and none of them said so: the operator
            pressed Send, the item left the queue, and it looked handled. They
            found out by opening Outlook and seeing nothing. A terminal failure
            with a recorded cause must be the loudest thing on the card. */}
        {item.delivery_failure ? (
          <p className="unanswered-email-failure" role="alert">
            <strong>This reply was not delivered.</strong> {item.delivery_failure}
          </p>
        ) : null}
        <p>
          “{item.title}”.
          {/* How many people one press of Send reaches. Naming only the earliest
              sender made a seven-thread send look identical to a one-thread
              send, and this Hive has sent to seven. */}
          {item.thread_count > 1 ? ` Sending answers all ${item.thread_count} original threads at once.` : ""}
          {/* SENDING IS NOT WAITING, and it is not nothing either. A reply
              mid-flight lives on a row in state 'queued', so a card looking
              only for a draft called it "No reply has been written" while
              Sent Items was filling up in front of the operator. */}
          {item.sending
            ? " This reply is being sent now."
            : uncertain ? " Swarm could not confirm whether this reply was delivered."
            : item.draft_body ? " A reply is written and waiting for you to read it." : item.drafted ? " A reply is written but was never sent." : " No reply has been written."}
        </p>
        {uncertain ? (
          <p className="unanswered-email-failure" role="alert">
            <strong>Microsoft may already have delivered this reply.</strong> Check Sent Items in Outlook before you retry, or the person may get it twice.
          </p>
        ) : null}
        {/* WHY IT IS EMPTY, when the board knows why. The operator opened a
            completed email task, found a blank textarea, and read it as an
            instruction: "It wanted me to write it? that isn't how this should
            be working." They were right that it should not. A worker had
            written a reply and the gate refused it, and that refusal died with
            the worker's turn, so the card had nothing to say except that the
            box was empty. Writing the reply is the worker's job; reading it is
            theirs. When no reply exists, say what happened instead of leaving
            the blank to imply an instruction. */}
        {!item.sending && !item.draft_body && !item.drafted && item.no_reply_reason ? (
          <p className="unanswered-email-no-reply">{item.no_reply_reason} Writing it is the worker&rsquo;s job, not yours.</p>
        ) : null}
        {editing ? (
          <div className="unanswered-email-editor">
            <label>
              <span className="field-caption">Reply to {item.sender_name || item.sender_address}</span>
              <textarea rows={12} value={body} disabled={busy} onChange={(event) => { setBody(event.target.value); setUndoBody(null); }} />
            </label>
            <small>{countWords(body)} words</small>
            {onReviseReply ? (
              <div className="unanswered-email-revise">
                <label>
                  <span className="field-caption">Ask Claude to change it</span>
                  <input
                    type="text"
                    value={instruction}
                    disabled={busy}
                    placeholder="Halve it · warmer · drop the second paragraph"
                    onChange={(event) => setInstruction(event.target.value)}
                    onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); void revise(); } }}
                  />
                </label>
                <button className="secondary-button" type="button" disabled={busy || !instruction.trim()} onClick={() => void revise()}>Revise</button>
                {/* Undo is offered only when there is something to undo, so it
                    never implies a history that does not exist. */}
                {undoBody !== null ? (
                  <button className="text-button" type="button" disabled={busy} onClick={() => { setBody(undoBody); setUndoBody(null); }}>Undo revision</button>
                ) : null}
              </div>
            ) : null}
          </div>
        ) : item.draft_body ? (
          <blockquote className="unanswered-email-draft">
            {item.draft_body}
            <small>{countWords(item.draft_body)} words</small>
          </blockquote>
        ) : null}
        {needsDraftPermission ? (
          <p className="unanswered-email-failure" role="alert">
            <strong>Outlook has not let Swarm save drafts yet.</strong> Reconnect this mailbox once to allow it. Sending works as before.
            {onReconnectForDrafts ? (
              <> <button className="text-button" type="button" disabled={busy} onClick={onReconnectForDrafts}>Reconnect for drafts</button></>
            ) : null}
          </p>
        ) : null}
      </div>
      <div className="queen-attention-actions">
        {editing ? (
          <>
            <button
              className="primary-action"
              type="button"
              disabled={busy || !body.trim() || body.trim() === (item.draft_body ?? "")}
              onClick={() => { onSaveReply?.(item.task_id, body.trim()); setEditing(false); }}
            >Save changes</button>
            <button
              className="secondary-button"
              type="button"
              disabled={busy}
              onClick={() => { setBody(item.draft_body ?? ""); setEditing(false); }}
            >Cancel</button>
          </>
        ) : confirmingDontSend ? (
          <>
            <span className="field-caption">Close this without a reply? Nothing is sent.</span>
            <button className="primary-action" type="button" disabled={busy} onClick={() => { setConfirmingDontSend(false); onDontSend?.(item.task_id); }}>
              Yes, don&rsquo;t send
            </button>
            <button className="secondary-button" type="button" disabled={busy} onClick={() => setConfirmingDontSend(false)}>Keep it</button>
          </>
        ) : (
          <>
            {item.draft_id && onSendReply ? (
              <button className="primary-action" type="button" disabled={busy} onClick={() => onSendReply(item.draft_id!)}>
                Send this reply
              </button>
            ) : null}
            {/* Nothing to press while it is going out. Offering Edit or Write
                here invites the operator to change or replace text that has
                already left, or is leaving. */}
            {uncertain && onRetryReply ? (
              <button className="primary-action" type="button" disabled={busy} onClick={() => onRetryReply(item.uncertain_reply_id!)}>
                Retry the send
              </button>
            ) : null}
            {item.draft_body && onSaveAsDraft && settled ? (
              <button className="secondary-button" type="button" disabled={busy} onClick={() => void saveAsDraft()}>
                Save as draft in Outlook
              </button>
            ) : null}
            {canEditHere && settled ? (
              <button className="secondary-button" type="button" disabled={busy} onClick={() => { setBody(item.draft_body ?? ""); setEditing(true); }}>
                {item.draft_body ? "Edit here" : "Write the reply"}
              </button>
            ) : null}
            {onDontSend && settled ? (
              <button className="text-button" type="button" disabled={busy} onClick={() => setConfirmingDontSend(true)}>
                Don&rsquo;t send
              </button>
            ) : null}
            {/* Still a route to the task itself, for everything a reply editor
                is not — the original thread, its attachments, the history. It
                is no longer the only way to change a word. */}
            <button className={item.draft_id || uncertain ? "text-button" : "primary-action"} type="button" onClick={() => onOpenTask(item.task_id)}>
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
