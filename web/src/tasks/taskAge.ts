const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/**
 * How long a task has been waiting, short enough to sit beside its state.
 *
 * Relative rather than a date, because the question a board answers at a glance
 * is "which of these has gone wrong", and a five-day-old draft reads as wrong
 * where "19 Aug" reads as a fact to work out. The operator found exactly that
 * set by opening ninety tasks one at a time.
 *
 * Coarse on purpose: nothing here is improved by knowing a task is 37 minutes
 * old rather than 38, and a row has no room to say so.
 */
export function taskAge(createdAt: number, now: number): string {
  const seconds = Math.max(0, Math.floor(now / 1000) - createdAt);
  if (seconds < MINUTE) return "just now";
  if (seconds < HOUR) return `${Math.floor(seconds / MINUTE)}m`;
  if (seconds < DAY) return `${Math.floor(seconds / HOUR)}h`;
  return `${Math.floor(seconds / DAY)}d`;
}

/** The exact moment, for the tooltip. The short form is for scanning. */
export function taskAgeTitle(createdAt: number): string {
  return `Created ${new Date(createdAt * 1000).toLocaleString()}`;
}
