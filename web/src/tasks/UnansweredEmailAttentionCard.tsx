import type { UnansweredEmailTask } from "../api";
import BeeMascot from "../brand/BeeMascot";

/** Words of the draft shown before it is cut, so a card can be scanned. */
const PREVIEW_WORDS = 45;

/**
 * Completed email tasks whose requester has not been answered — ONE CARD EACH.
 *
 * A task imported from email carries a person waiting on it, and finishing the
 * work tells them nothing: the reply is a separate, deliberate step. Nothing
 * used to notice when that step never happened, so a worker could close the
 * task and the person who wrote in heard nothing at all.
 *
 * ONE CARD EACH is the correction, and it is the whole point of this component
 * rather than a detail of it. This used to render `const [first] = awaiting`
 * and reduce every other waiting person to the phrase "and 2 others like it" —
 * unreadable, unsendable, unreachable. The only route to the second was to send
 * the first, wait for the list to change, and look again. Meanwhile the queue
 * counted the whole group as a single item, so three people waiting showed as
 * one. The operator asked for exactly this: "separate each email into a separate
 * for you item that I could quickly scan through and approve or edit for
 * sending."
 *
 * SCANNING IS WHY THE DRAFT IS TRUNCATED HERE. The old card printed the full
 * body inline, and the drafts being written are 273 to 627 words — measured on
 * the operator's own Hive, on the day they called one "way too long". A card
 * you have to scroll is not a card you can scan, and burying the other two
 * people below one wall of text is how they became invisible. The word count is
 * shown rather than implied, because "how long is this" is the first thing being
 * triaged.
 *
 * Reading the words is the operator's part; whether the work is actually
 * running in production is the worker's, recorded as deployment evidence.
 */
export default function UnansweredEmailAttentionCard({ awaiting, busy, onOpenTask, onSendReply }: {
  awaiting: UnansweredEmailTask[];
  busy?: boolean;
  onOpenTask: (taskId: string) => void;
  onSendReply?: (replyId: string) => void;
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
        />
      ))}
    </>
  );
}

function UnansweredEmailCard({ item, busy, onOpenTask, onSendReply }: {
  item: UnansweredEmailTask;
  busy?: boolean;
  onOpenTask: (taskId: string) => void;
  onSendReply?: (replyId: string) => void;
}) {
  const heading = `unanswered-email-${item.task_id}`;
  const draft = item.draft_body ? summarise(item.draft_body) : null;
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
        {draft ? (
          <blockquote className="unanswered-email-draft">
            {draft.preview}
            {/* The count is the triage signal, so it is stated even when the
                draft is short enough to show whole. */}
            <small>{draft.words} words{draft.truncated ? " · shown in part" : ""}</small>
          </blockquote>
        ) : null}
      </div>
      <div className="queen-attention-actions">
        {item.draft_id && onSendReply ? (
          <button className="primary-action" type="button" disabled={busy} onClick={() => onSendReply(item.draft_id!)}>
            Send this reply
          </button>
        ) : null}
        <button className={item.draft_id ? "secondary-button" : "primary-action"} type="button" onClick={() => onOpenTask(item.task_id)}>
          {item.draft_id ? "Edit first" : "Open the task"}
        </button>
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

/**
 * Enough of the draft to decide on, and its length.
 *
 * Splitting on whitespace rather than counting characters, because the question
 * being answered is "is this too long to send as written", and a reader judges
 * that in words.
 */
function summarise(body: string): { preview: string; words: number; truncated: boolean } {
  const words = body.trim().split(/\s+/).filter(Boolean);
  const truncated = words.length > PREVIEW_WORDS;
  return {
    preview: truncated ? `${words.slice(0, PREVIEW_WORDS).join(" ")}…` : body.trim(),
    words: words.length,
    truncated,
  };
}
