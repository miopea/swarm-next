# ADR 0069: One tab-owned terminal composer draft

Status: Accepted implementation of the approved TERM-01/MOB-01 scope; real-device
acceptance remains pending.

## Decision

The browser tab owns one composer draft, outside React and terminal renderer
lifetime. It carries an immutable session ID, exact bounded text and an uncertain
submission marker. It is not a per-worker draft map or a delivery queue. Switching
sessions cannot migrate it; another session offers explicit discard before editing.
Returning to its originating session restores it without sending anything.

Preserve the existing 16,384 UTF-16-unit text bound. The serialized envelope is
bounded at six times that count plus 1,024 characters, with a 256-character session
ID limit and at most 32 subscribers. One tab/sessionStorage entry is retained until
sent/cleared, explicit logout, credential replacement, or browser storage disposal.
No indefinite history or cross-device synchronization is introduced.

Ordinary edits stay in memory. View teardown, pagehide and hidden visibility flush
to sessionStorage. Before any Send write, persist the uncertainty marker. A known
local refusal clears that marker; interruption leaves it in place. Restoring an
uncertain draft requires operator inspection before editing/resending. Browser
acceptance of Enter retains the existing clear-draft behavior, not a claim of
provider consumption. Nothing automatically replays input or reconstructs raw keys.

Storage failure keeps the in-memory draft and shows a durability warning. Abrupt
termination before a lifecycle flush can lose recent edits; storage is not a
transactional guarantee. Logout attempts to clear storage, subject to browser access.
No draft enters diagnostics, Dogfood captures or source records before explicit Send.

This qualifies ADR 0065's statement that its source-recording path stores nothing
in the browser: the separate recoverable draft is local UI state, not a durable
authorship or consumption receipt. Queen does not read unsent drafts.

## Verification

Prove exact-text restoration, single-session ownership, explicit discard, bounds,
storage refusal, no per-edit storage writes, uncertainty surviving paste/Enter
interruption, no replay, renderer-reset preservation and logout clearing. Real PWA
eviction/reload and mobile interaction remain required acceptance evidence.
