# Browser dogfood acceptance

Status: **Passed baseline**

The local acceptance runtime used the fixed Swarm UI contract at port `8766`,
an isolated database and history directory, and three real Claude sessions:
Queen, Forager, and Pollen. No legacy or operator production data participated.

## Verified journeys

- First worker creation opened a usable terminal surface without a blank page.
- Nine alternating desktop selections preserved each worker's exact session ID.
- Reload stayed unlocked, restored all three durable worker profiles, selected
  the always-active Queen, and reattached the original Queen session.
- A task created through the API changed the open browser count from one to two
  without pressing Refresh, proving the resumable control-room invalidation
  path in the rendered app.
- Settings reported connected live updates and healthy API, database,
  terminal-host, and provider layers.
- The generated report included three session IDs and 11 recent content-free
  transitions while excluding the test credential, workspace paths, and worker
  names.
- Light- and dark-theme visual inspection found and fixed an unreadable
  light-theme diagnostic preview foreground.
- A 412 by 915 mobile pass found and fixed a hidden-worker-roster blocker. The
  compact mobile worker strip can select Forager, attach its original session,
  and remains available after reload.
- Mobile terminal controls provide bounded long-form composition, exact slash
  commands, bracketed multiline paste, Enter/Esc/Tab, arrow navigation, and
  permission-mode cycling without requiring a hardware keyboard.
- A task-history pass created, edited, transitioned, and assigned work in the
  rendered app. The bounded timeline remained readable at 412 by 915 and
  returned unchanged after a full reload with no console warnings or errors.
- A task-ordering pass moved two open tasks through the authenticated durable
  reorder contract. The new order survived reload, and compact Earlier/Later
  controls remained visible at 412 by 915 with a clean console.
- Task cards now match the worker roster's accessible action-menu pattern. The
  visible trigger and right-click open the same menu, Escape dismisses it, and
  editing, history, ordering, and exceptional state actions remain reachable.
  A 412 by 915 pass confirmed the reduced mobile action row and readable menu.
- A global feedback action now captures the operator's expected and observed
  outcome alongside content-free surface, selected-session, subsystem,
  resource, history, and recent-transition evidence. Desktop and 412 by 915
  mobile passes verified preview, copy, Escape dismissal, internal scrolling,
  and the exclusion of tokens, worker names, paths, terminal output, task text,
  and raw provider errors from automatic collection.

## Development port contract

Vite now binds strictly to `8766`; startup fails instead of silently moving to
another port. This keeps local development aligned with the SSH and Cloudflare
mapping used by the dogfood environment. API proxy traffic remains on the local
API port `8765`.

## Remaining evidence

This pass proves functional redraw and recovery, not long-duration performance.
The multi-day soak and outbound feedback submission transport remain separate
promotion work.

## Dogfood usability correction (2026-08-12)

An isolated local runtime and the rendered 1280 by 720 browser surface verified
the first correction batch after operator dogfooding:

- A named sleeping worker was created from a friendly repository catalog; no
  workspace path entry appeared. The backend also rejected an unlisted path
  with `422 unknown_workspace` and prevents a repository from belonging to two
  workers.
- Starting and switching workers moved focus to the real xterm input. The
  selected worker, repository affinity, exact session, task, and unlocked tab
  all survived a full navigation reload.
- The live Claude screen rendered distinct ANSI colors after the PTY advertised
  `TERM=xterm-256color` and `COLORTERM=truecolor`.
- An authenticated image clipboard upload produced a private content-addressed
  file for the selected session. File count, total bytes, per-file bytes, age,
  type signature, and Unix permissions remain bounded and tested.
- Task creation selected the durable worker by name and showed `Daisy ·
  budgetbug`; neither task creation nor assignment asked the operator for a
  filesystem path.
- A root render boundary replaces a blank page with a safe reload action and a
  bounded content-free failure marker that survives refresh for the diagnostic
  bundle.
- Notification tests are device-scoped, mobile never requests desktop
  lock-detection permission, and the mobile policy control uses readable theme
  cards instead of the platform's oversized select sheet.

The mobile CSS correction makes the worker surface a fixed-height terminal-first
layout: compact brand/navigation, a slim horizontal worker strip, a 52-pixel
worker header, expanded terminal space, and compact D-pad controls. Unit and
production-build evidence cover these rules in this local pass; a physical
Android screenshot remains required after the batch reaches the dogfood server.

## Durable roster maintenance (2026-08-12)

An isolated runtime on port `8767` exercised the production-built web surface against
the newly built API contract rather than mocked data:

- Settings renamed Daisy to Marigold and enabled her always-active policy while
  preserving the assigned `budgetbug` repository and provider conversation.
- A full reload retained the new name and policy.
- Desktop settings kept edit and ordering controls on one calm row with no
  horizontal overflow. Native drag ordering supplements visible arrow actions.
- At 412 by 915, the edit form remained readable, the repository boundary was
  explicit, touch ordering moved Poppy before Marigold, and the order survived
  reload with zero horizontal overflow.
- Browser logs contained no warnings or errors during edit, reorder, and reload.

## Repository path completion (2026-08-12)

The isolated runtime used nested `personal` and `rcg` folders to verify the
actual production web build against the rebuilt API binary:

- Typing `personal/bud` produced the nested `budgetbug` repository and Enter
  completed the canonical absolute path before creating Daisy.
- A directly typed existing path is accepted even when it is outside the
  bounded suggestion result, while the server rejects non-existent paths,
  symlinks, and paths outside configured workspace roots.
- At 412 by 915, `rcg/pub` displayed `public-website` first with its full path
  as secondary context; the popup stayed anchored with zero horizontal
  overflow.
- At 1440 by 900, folder and repository results remained visually distinct,
  keyboard reachable, and free of browser warnings or errors.
- The push worker now activates updates immediately, bypasses the HTTP cache
  when explicitly registered, and uses the current Queen launcher artwork for
  notification icon and badge requests.

## Release terminal geometry recovery (2026-08-12)

Operator dogfooding exposed a production-only terminal corruption: optimized
terminal-host builds resized the PTY but compiled the matching canonical-screen
resize out because the state transition lived inside `debug_assert!`. Durable
snapshots therefore remained at `80×24` while Claude painted for the wider PTY.

- The canonical resize now executes in every build profile; the assertion only
  verifies its result.
- CI runs the PTY/canonical-dimension regression in the optimized release
  profile so debug-only behavior cannot satisfy this acceptance gate.
- Browser resize events are coalesced and publish xterm's settled dimensions,
  instead of discarding an event while xterm is updating its public geometry.
- A fresh optimized host and production web build changed a new worker from the
  `80×24` launch default to `155×41`, survived reload without corruption, then
  changed to `43×24` at 412 by 915 with readable output and zero overflow.

## Guarded coordination submission recovery (2026-08-12)

The first production task-assignment dogfood pass created one isolated task,
assigned it to a quiet durable worker, and observed the complete brief rendered
at Claude's prompt. The dispatch ledger reported Delivered, but Claude had not
accepted the prompt. The same behavior left the worker's Review handoff sitting
unsubmitted at Queen's prompt.

- A separately submitted Enter advanced the worker from Ready to Active to
  Review and delivered one durable outcome to Queen.
- A separately submitted Enter let Queen inspect the handoff and approve the
  task as Completed.
- Splitting text and Enter into immediate terminal-host requests still allowed
  the PTY to coalesce both events. Coordination delivery now writes sanitized
  prompt text, waits for its bounded identity marker in host-owned canonical
  output across a bounded series of real output advances, and only then sends
  Enter for task briefs, decision outcomes, and Queen handoffs. Delivery is
  recorded only after Enter is acknowledged; a stalled render or any post-write
  ambiguity fails closed as Uncertain and is never replayed.
- The real terminal-host assignment integration, strict workspace lint, and all
  164 Rust tests pass with the corrected submission boundary.
- Release `0.1.0-9c575f070c44` then proved the production path without manual
  terminal input: an operator engagement kept the task Queued; expiry delivered
  that same assignment once; the worker moved Ready to Active to Review; Queen
  received the guarded outcome and approved Completed. A subsequent API restart
  preserved the completed task, both worker session IDs, and terminal-host PID.
