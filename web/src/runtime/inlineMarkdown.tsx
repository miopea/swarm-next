import type { ReactNode } from "react";

/**
 * The little bit of Markdown release notes actually use.
 *
 * Release notes are written in RELEASE_NOTES.md and carried into the artifact
 * verbatim, so whatever the releaser typed arrives here as characters. Before
 * 1.0.0 nobody had used any markup and it did not matter; 1.0.0 leaned on
 * `**bold**` for the lead of every bullet and the panel printed the asterisks.
 *
 * BOLD AND CODE, AND DELIBERATELY NOT ITALIC. `_` and `*` are single characters
 * inside identifiers this product says out loud — email_reply_deliveries,
 * user_version, swarm_transition_task — and an italic rule would eat the middle
 * of them and leave the text quietly wrong. Bold needs a doubled marker and code
 * needs backticks, and neither occurs by accident in the notes.
 *
 * Returns nodes rather than HTML: nothing here is ever handed to
 * dangerouslySetInnerHTML, so a note cannot inject markup no matter who wrote it.
 */
export type InlineToken =
  | { kind: "text"; value: string }
  | { kind: "bold"; value: string }
  | { kind: "code"; value: string };

const PATTERN = /\*\*([^*]+)\*\*|`([^`]+)`/g;

export function tokenizeInline(source: string): InlineToken[] {
  const tokens: InlineToken[] = [];
  let cursor = 0;
  for (const match of source.matchAll(PATTERN)) {
    const at = match.index;
    if (at > cursor) tokens.push({ kind: "text", value: source.slice(cursor, at) });
    if (match[1] !== undefined) tokens.push({ kind: "bold", value: match[1] });
    else if (match[2] !== undefined) tokens.push({ kind: "code", value: match[2] });
    cursor = at + match[0].length;
  }
  if (cursor < source.length) tokens.push({ kind: "text", value: source.slice(cursor) });
  return tokens;
}

/**
 * A commit subject, read as a sentence.
 *
 * Conventional-commit subjects are written lowercase to follow a verb, which
 * reads correctly in a git log and reads like a fragment in a list. The capital
 * goes on the first token whatever KIND it is: 1.0.0's bullets open with bold,
 * and a version that capitalised the raw string would have been capitalising an
 * asterisk and leaving the visible first letter alone.
 *
 * Only the first character is touched. Rewording is the releaser's job.
 */
export function capitalizeFirst(tokens: InlineToken[]): InlineToken[] {
  const first = tokens[0];
  if (!first || first.kind === "code" || first.value.length === 0) return tokens;
  const capitalized = first.value.charAt(0).toUpperCase() + first.value.slice(1);
  return [{ ...first, value: capitalized }, ...tokens.slice(1)];
}

export function renderInline(source: string, keyPrefix: string): ReactNode[] {
  return capitalizeFirst(tokenizeInline(source)).map((token, index) => {
    const key = `${keyPrefix}-${index}`;
    if (token.kind === "bold") return <strong key={key}>{token.value}</strong>;
    if (token.kind === "code") return <code key={key}>{token.value}</code>;
    return <span key={key}>{token.value}</span>;
  });
}
