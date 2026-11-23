# 0187 — A command card keeps the script it runs

- Status: Accepted
- Date: 2026-11-22
- Decided by: the operator (2026-11-22), answering "Revised scopes": store the
  one script the card names, size-capped and scanned for secrets; show its text
  on the card and in Queen's card list; re-check its sha256 when Allow is
  pressed. Not every file it calls, and no diff for now.
- Builds on: ADR 0129 (a command runs once, bound to its script's hash),
  ADR 0158 and 0165 (Queen grants some cards).

## Context

A command card that runs a local script named the script's path and the
sha256 Swarm took of it when the card was filed (ADR 0129). Nobody answering
the card was shown the script.

- In the 14 days to 2026-11-22 the operator answered 288 cards that ran a
  script and Queen 371, each from a path and a hash.
- 629 of 693 script paths were in a worker's scratchpad, which the operator
  cannot open from Needs you, and Queen had to read the file herself before
  granting (ADR 0165).
- A changed script was already refused, but only when the command ran: the
  approval was spent on nothing, and the worker learned it at run time.
- Files a script calls cannot be found reliably, and ADR 0129 accepts that.

## Decision

1. **The card keeps the text it hashed.** When a card is filed, the script is
   read once: the sha256 and the text both come from that read, so the text a
   card shows is always the text its hash pins. Swarm keeps the text when it is
   UTF-8, at most 64 KiB, holds no characters that hide or reorder text, and
   holds nothing that looks like a secret. Otherwise it keeps no text and says
   why, and the card still pins the path and hash as before:
   - over the cap, with the size;
   - not text;
   - a line with hiding characters, by line number;
   - a line that looks like a secret, by line number and kind: a private key,
     a GitHub, Slack, Anthropic, OpenAI or 1Password token, an AWS access key,
     a JSON web token, a URL with a password, or a value written into the
     script for a password, secret, token or key name. The value is never
     kept or shown. A reference to a secret is not one: `op://`, `$VAR`,
     `${VAR}` and `$(op read ...)` pass.
   Filing is never refused for this; see the open question below.
2. **Whoever answers reads it.**
   - The operator opens "Read the script" on the card in Needs you. The text
     is fetched then, from `GET /api/v1/decisions/{id}/script`, so the list of
     cards does not carry every script.
   - `swarm_list_command_cards` carries the same for each of Queen's cards.
   - A card filed before this release says it kept no copy.
3. **Allow re-checks the hash.** When the operator presses Allow, or Queen
   grants a card, Swarm hashes the file again. If it is gone or its hash
   differs, that Allow is refused with both hashes and nothing is granted: the
   card stays pending, to be denied and filed again for the script as it is
   now. The check when the command runs stays.

## Where it lives

- The read, the snapshot and the scan are in the application
  (`swarm-application::command_scripts`). The API's own hashing moved there, so
  filing, Allow and run time share one reader.
- The text and the reason are two nullable columns on `decision_requests`
  (schema 234). They are not on the shared decision record, so the worker
  engine's build does not change.

## Consequences

- A schema migration, 233 to 234: two nullable columns, nothing rewritten.
- The scan is a set of patterns, so it misses secrets of other shapes and
  sometimes withholds a script that holds none. A miss shows the text to the
  same people who could already read the file on this machine; a false alarm
  shows the reason instead of the text.

## Open question

Should Swarm refuse to file a card whose script looks like it holds a secret,
so the worker reads it from 1Password instead? Not chosen silently: it changes
how workers file cards. Recorded in `docs/09-open-questions.md`.

## Evidence

- Application tests:
  - each kind of secret is found by line and never shown;
  - references and near misses pass;
  - the cap, non-UTF-8 and hiding characters;
  - one read pins the hash and the text, and a long script is still hashed
    whole;
  - the operator's Allow refused for a changed or vanished script, and allowed
    when unchanged;
  - Queen's grant refused when changed, allowed once restored.
- Persistence tests: the text and the reason round-trip, a card with no
  script and an unknown card answer as such, and malformed snapshots are
  refused.
- An HTTP test: the operator reads the script, unauthenticated reads are
  refused, a card with no script and an unknown card answer as such, and an
  Allow of a changed script is a 400 that leaves the card pending.
- An MCP test: Queen's list carries the text, withholds a token line without
  showing it, and her grant is refused for a changed script.
- Web tests: the script is fetched only when opened, its text and line count
  shown, a withheld reason shown, and a failed load says so. Looked at in the
  harness at 1280, 768 (dark) and 390 wide, with no sideways scroll at 390.

## Not proven

- No Hive has run it.
