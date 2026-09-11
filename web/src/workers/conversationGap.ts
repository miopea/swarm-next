/**
 * How much newer an unsaved transcript is than the saved one, in words.
 *
 * ⚠️ THIS REPLACES TWO RAW ISO STAMPS SIDE BY SIDE. The panel used to print
 * "Saved conversation last entry: 2026-09-11T15:24:42.325Z · Newest history:
 * 2026-09-11T15:25:14.367Z" and leave the reader to subtract them. Operator,
 * 2026-09-11: "The data in the boxes are not clear."
 *
 * The gap is the whole point — thirty seconds means the worker kept talking
 * after the pin, a week means the pin is to something old. The absolute
 * timestamps answered neither question without arithmetic.
 */
export function conversationGap(
  pinnedLastEntry: string | null | undefined,
  newestLastEntry: string | null | undefined,
): string {
  if (!pinnedLastEntry) return "and the saved conversation has no recorded entry";
  const pinned = Date.parse(pinnedLastEntry);
  const newest = Date.parse(newestLastEntry ?? "");
  // Unparseable is not zero. Saying "at the same moment" about a stamp we could
  // not read would be a confident wrong answer.
  if (Number.isNaN(pinned) || Number.isNaN(newest)) return "at an unreadable time";
  const seconds = Math.round((newest - pinned) / 1000);
  if (seconds <= 0) return "no later than the saved conversation";
  return `${humanGap(seconds)} after the saved conversation's last entry`;
}

function humanGap(seconds: number): string {
  if (seconds < 60) return `${seconds} second${seconds === 1 ? "" : "s"}`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? "" : "s"}`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours} hour${hours === 1 ? "" : "s"}`;
  const days = Math.round(hours / 24);
  return `${days} day${days === 1 ? "" : "s"}`;
}
