const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/**
 * How long a task has been waiting.
 *
 * This used to be what the row displayed, on the reasoning that "5d" reads as
 * wrong where a date reads as a fact to work out. The operator overruled it
 * after living with it — "the Age is useless, we need a created date" — so the
 * date is what shows and this is the tooltip. The argument was not worthless:
 * elapsed time is the thing a date makes you compute, which is why it is still
 * here rather than deleted.
 *
 * Coarse on purpose: nothing is improved by knowing a task is 37 minutes old
 * rather than 38.
 */
export function taskAge(createdAt: number, now: number): string {
  const seconds = Math.max(0, Math.floor(now / 1000) - createdAt);
  if (seconds < MINUTE) return "just now";
  if (seconds < HOUR) return `${Math.floor(seconds / MINUTE)}m`;
  if (seconds < DAY) return `${Math.floor(seconds / HOUR)}h`;
  return `${Math.floor(seconds / DAY)}d`;
}

/**
 * When the task was created, short enough to sit beside its state.
 *
 * The year is shown only when it is not the current one: on a board where
 * nearly everything is from this year, repeating it on every row is the same
 * kind of constant that made the source column worth reclaiming.
 */
export function taskCreatedOn(createdAt: number, now: number): string {
  const created = new Date(createdAt * 1000);
  const sameYear = created.getFullYear() === new Date(now).getFullYear();
  return created.toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    ...(sameYear ? {} : { year: "numeric" }),
  });
}

/** The exact moment and the elapsed time, for the tooltip. */
export function taskAgeTitle(createdAt: number, now: number): string {
  return `Created ${new Date(createdAt * 1000).toLocaleString()} · ${taskAge(createdAt, now)} ago`;
}
