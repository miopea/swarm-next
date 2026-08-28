/**
 * The task title limit, counted the way the server counts it.
 *
 * WHY THIS EXISTS. The server validates `title.len() > 240` in Rust, which is
 * 240 UTF-8 BYTES (swarm-persistence MAX_TASK_TITLE_BYTES). Every title field
 * in the app carried `maxLength={240}`, which counts UTF-16 code units. The two
 * agree only for ASCII.
 *
 * Email subjects are the worst case and are exactly where this was reported: a
 * mail client writes a curly apostrophe (3 bytes), an em dash (3 bytes) or an
 * accented sender's name (2 bytes each), so a subject the field calls 240 long
 * can be well over 240 bytes. The operator sees a field that is not complaining,
 * a title that looks short enough, and a 400 that says "1 to 240 bytes".
 *
 * WORSE, AND THE ACTUAL REPORTED BUG: `maxLength` constrains TYPING only. It
 * does nothing to a value set programmatically, and the email intake sets the
 * title from the subject with `setTitle(...)`. So the auto-filled title was not
 * bounded at all — no amount of editing inside a field whose limit never
 * applied was going to clear the error, which is precisely what the operator
 * reported: "Even when I edit the this error appears."
 */
export const TITLE_BYTE_LIMIT = 240;

const encoder = new TextEncoder();

/** What the server will measure. */
export function titleByteLength(value: string): number {
  return encoder.encode(value).length;
}

export function titleFits(value: string): boolean {
  return titleByteLength(value.trim()) <= TITLE_BYTE_LIMIT;
}

/**
 * Split into user-perceived characters so truncation never lands mid-character.
 *
 * Cutting a byte array at 240 can slice a multi-byte code point in half, and
 * cutting between code points can split an emoji's ZWJ sequence into pieces.
 * Neither is something to hand back to someone as their title.
 */
function graphemes(value: string): string[] {
  const segmenter = (
    Intl as unknown as {
      Segmenter?: new (
        locale?: string,
        options?: { granularity: string },
      ) => { segment: (value: string) => Iterable<{ segment: string }> };
    }
  ).Segmenter;
  if (!segmenter) return [...value];
  return [...new segmenter(undefined, { granularity: "grapheme" }).segment(value)].map(
    (part) => part.segment,
  );
}

/**
 * The longest leading part of `value` that fits, with an ellipsis when anything
 * was dropped.
 *
 * VISIBLY shortened, on purpose. Silently trimming a title loses information
 * the operator cannot see they have lost; the ellipsis is what tells them the
 * subject was longer than the field can carry, so they can rewrite it rather
 * than wonder.
 */
export function clampTitleToBytes(value: string, limit: number = TITLE_BYTE_LIMIT): string {
  if (titleByteLength(value) <= limit) return value;
  const ellipsis = "…";
  const budget = limit - titleByteLength(ellipsis);
  let used = 0;
  let out = "";
  for (const cluster of graphemes(value)) {
    const size = titleByteLength(cluster);
    if (used + size > budget) break;
    used += size;
    out += cluster;
  }
  return `${out.trimEnd()}${ellipsis}`;
}
