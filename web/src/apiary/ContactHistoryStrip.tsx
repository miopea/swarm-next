import type { ContactHistory } from "../api";

/** The most gaps named in words; the strip still shows every one. */
const NAMED_GAPS = 3;
const MAX_SLOTS = 288;

/**
 * When a member Hive reached its Keeper over the last day (ADR 0114), so a drop
 * that keeps coming back is visible at a glance. On 2026-10-01 a member failed
 * DNS from 08:08 to 11:15 UTC and the Keeper found out from logs.
 *
 * Shown only when a Hive is opened, never on the roster, and it raises nothing
 * by itself: it is the record, not an alarm. The words say what the colours
 * do, so neither is needed to read the other.
 */
export default function ContactHistoryStrip({ history }: { history: ContactHistory }) {
  const slots: number[] = [];
  // Bounded whatever the reply says: a day is 288 slots.
  for (let at = history.from; at <= history.until && slots.length < MAX_SLOTS; at += history.slot_seconds) slots.push(at);
  const contacted = new Set(history.contacted);
  // The slot now falls in may simply not have come round yet.
  const settled = contacted.has(history.until) ? slots : slots.slice(0, -1);
  const gaps = gapsIn(settled, contacted, history.slot_seconds);
  const summary = gaps.length === 0
    ? "Reached the Keeper throughout the last 24 hours."
    : `Unreachable ${gaps.slice(-NAMED_GAPS).map(([from, until]) => `${clock(from)}–${clock(until)}`).join(", ")}${gaps.length > NAMED_GAPS ? `, and ${gaps.length - NAMED_GAPS} more` : ""} in the last 24 hours.`;
  return (
    <figure className="contact-history">
      <figcaption>{summary}</figcaption>
      <div className="contact-history-strip" role="img" aria-label={summary}>
        {slots.map((at) => <span key={at} className={contacted.has(at) ? "is-contacted" : undefined} />)}
      </div>
      <div className="contact-history-axis" aria-hidden="true"><span>24 hours ago</span><span>Now</span></div>
    </figure>
  );
}

/** Each run of slots with no contact, as [its start, the end of its last slot]. */
function gapsIn(slots: number[], contacted: Set<number>, width: number): [number, number][] {
  const gaps: [number, number][] = [];
  let start: number | undefined;
  for (const at of slots) {
    if (!contacted.has(at)) start ??= at;
    else if (start !== undefined) { gaps.push([start, at]); start = undefined; }
  }
  if (start !== undefined && slots.length) gaps.push([start, slots[slots.length - 1] + width]);
  return gaps;
}

function clock(seconds: number): string {
  return new Date(seconds * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
