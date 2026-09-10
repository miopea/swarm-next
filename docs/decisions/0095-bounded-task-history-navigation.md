# ADR 0095: Bounded task-history navigation

Status: Accepted within approved UI-4 full-history-on-demand scope.

Task activity already has immutable, ordered sequence identities and bounded
reads, but the browser exposes only the latest 30 events. Earlier decisions
must be reachable without increasing the default payload or accumulating pages.

Add an optional exclusive `before` sequence to the authenticated task-activity
read. Persistence owns selection and still enforces the existing 100-event hard
limit and limit-plus-one truncation check. Existing callers continue to read the
latest page. Sequence cursors avoid offset drift when concurrent events arrive;
no schema, retention, task-state, permission or worker-lifecycle change occurs.

The browser holds only one 30-event page. Older activity replaces it; Latest
activity returns to current history. Reopening history starts at latest. Reads
use the existing cancellation/deadline owner. A failed older-page read is
retryable at that cursor, or the operator can return to latest. Empty older
history is explicit, not an implied complete absence of task history.

Validation must cover ordered/nonoverlapping pages, concurrent newer events,
unknown tasks, invalid cursor/limit and authentication, UI older/latest and
failure/retry, cancellation of late older replies, and rendered narrow/desktop
navigation. This closes access to retained history, not restoration of records
removed by retention or a backup restore.
