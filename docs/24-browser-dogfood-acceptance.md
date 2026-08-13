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

## Device-owned engagement and complete live handoff (2026-08-13)

Release `0.1.0-0927bbfb5f32` was exercised through the authenticated production
browser against an isolated Git repository and a newly configured durable
worker:

- Creating and starting `Dogfood Clover` opened the expected provider trust
  flow, then retained the worker as a named repository-owned profile.
- A high-priority draft selected the worker by name, moved through Ready and
  Active, and remained queued while the operator was engaged.
- Selecting Queen immediately removed `With you` from Clover without claiming
  Queen. The queued briefing then appeared once at Clover's prompt.
- Clover verified the authoritative assignment, performed the requested
  read-only inspection, moved the task to Review with a concise handoff, and
  Queen received that outcome once after the operator switched away.
- The rendered task card exposed `Worker briefed` and `Queen notified` before
  the operator completed the task. No manual Refresh was needed.
- Claude's first use of each scoped Swarm MCP command still requires the
  provider's own repository permission confirmation. This is intentional under
  ADR 0003; Swarm does not silently override provider permissions.
- The same pass found and corrected stale Settings shortcut labels so the
  displayed Alt+1 through Alt+4 mapping matches the running application.
- Repeated desktop switching preserved the exact Queen, Swarm Next, and Clover
  session IDs across nine selections. Reload then exposed a view mismatch: the
  engagement lease correctly survived on Clover while the UI defaulted to
  Queen. The client now restores the last selected live session so the visible
  terminal and `With you` state remain coherent after reload or PWA reopen.
- Mobile-sized testing at 412 by 915 preserved a readable terminal, a complete
  composer and D-pad, and zero document-level horizontal overflow. Selecting a
  worker focused terminal input immediately, and the mobile composer delivered
  a slash command without losing its controls. Completed assignments are now
  removed from the live roster so shipped work cannot masquerade as a worker's
  current task.
- Night Watch persisted through a production reload, Automatic returned the
  Hive to its active-device state, and notification policy moved to Every
  decision and back without a stale selection. The current browser honestly
  reports its blocked notification permission. Permission refusal is now
  expressed only by the inline lock/notification capability state, avoiding a
  contradictory global failure banner when the browser already completed or
  reconciled the operation.
- A live API-only restart replaced PID `658477` with `721890` while the
  terminal-host PID remained `400662`. Queen, Swarm Next, and Dogfood Clover
  reconnected under the same three session IDs before and after a browser
  reload. Eight samples over 105 seconds kept API memory between 3.2 and 3.8
  MiB and the host cgroup, including all three Claude processes, between 937.5
  and 938.1 MiB. This bounded sample rules out rapid growth; it does not replace
  the documented 24-hour promotion soak.
- Dogfood feedback now accepts a screenshot by paste, drop, or file selection,
  previews it locally, and records only its filename, media type, and byte size
  in the copied diagnostic text. Image bytes are neither inspected nor
  uploaded automatically; the operator keeps the image as a separate explicit
  attachment. Desktop and mobile CSS keep the preview actions inside the
  bounded feedback dialog.

## Provider readiness and bounded live soak (2026-08-13)

Release `0.1.0-b74ba072198d` added a private provider-capability contract from
the independently updated terminal host through the API to worker settings:

- New workers can select only coding providers whose executable is present in
  the terminal host's bounded service `PATH`. An older still-running host
  degrades explicitly to Claude available and Codex awaiting maintenance.
- The Codex launch adapter uses the installed CLI's current contracts: `codex`
  for a new repository conversation and `codex resume --last` for recovery.
  No Swarm permission override is added.
- The alpha server has Codex `0.147.0` installed and authenticated. Activation
  remains deferred until the three active Claude sessions reach a safe
  zero-session maintenance point; the API update preserved terminal-host PID
  `400662` and all running sessions.
- A reusable headless Edge acceptance pass now opens the deployed HTTPS app at
  1440 by 900 and 412 by 915, performs a real unlock, reloads to verify the
  durable browser session, opens Settings, rejects horizontal overflow, checks
  provider readiness, captures screenshots, and fails on authenticated-page
  console or runtime errors. Both sizes passed against this release.
- A 20-minute live sample spanning API updates kept API RSS between 2.5 and
  4.4 MiB. The terminal-host cgroup, including three Claude processes, remained
  between 920 and 927 MiB and ended at 925.7 MiB. This is bounded short-run
  evidence; it does not replace the 24-hour promotion soak.
- Release `0.1.0-400b5f82b1f3` migrated the live Hive to schema 18 and stored
  independent desktop and mobile presentation profiles. Existing local theme
  and terminal-key choices seed each profile once; later changes propagate
  through the control-room feed and are included in database backup/restore.
  The deployed browser gate passed both profiles with real desktop and Android
  user agents, durable authentication, no overflow, and no authenticated-page
  browser errors while preserving terminal-host PID `400662`.
- Release `0.1.0-ce34b21f093b` embeds its exact package release in API health
  and terminal-host status. The Settings runtime card now translates a safe
  version mismatch into `Update waiting · 3 active` instead of exposing
  release mechanics or implying the update failed. The packaged-runtime test
  proves both binaries match the bundle version, and the deployed desktop and
  Android browser gate asserts the maintenance state is present and readable.
- The same deployed desktop gate now downloads a fresh authenticated Hive
  backup, reads the entire artifact, verifies the SQLite file signature and a
  non-trivial bounded size, and deletes the browser's temporary download. The
  server integration test independently reopens the backup and runs SQLite
  integrity verification, so browser delivery and database validity are both
  covered without retaining private dogfood data.
- The reusable gate now navigates Needs you, Tasks, Workers, and Settings at
  both 1440 by 900 and an authentic Android 412 by 915 context. Every surface
  must remain inside the viewport, the worker check waits for a real mounted
  terminal rather than accepting its loading fallback, and each surface is
  captured for visual review. The deployed pass rendered all eight views with
  no authenticated-page console or runtime errors.
- Release `0.1.0-0fa81dbbe144` added an explicit worker-engine maintenance
  action for the always-active Queen case. Deployment retained host PID
  `400662` and all three sessions while starting the private systemd path
  watcher. The deployed desktop and Android gates each opened the second-step
  confirmation, verified its full copy and controls without horizontal
  overflow, and selected Not now; neither test stopped a worker. A separate
  real-PTY integration test performs the destructive half against an isolated
  old host and proves exact-version replacement, request cleanup, and durable
  recovery boundaries.
- Release `0.1.0-d6b334bf0c06` made private dogfood feedback reachable on the
  mobile control room instead of hiding it behind a desktop-only responsive
  rule. The deployed gate opened the feedback dialog at both sizes, entered a
  preview note, verified `Save to this Hive` was available, and closed without
  creating test data. All eight primary views remained within their viewport,
  the backup retained a valid SQLite signature, no authenticated-page browser
  errors occurred, and terminal-host PID `400662` plus all three live sessions
  were preserved.
- Release `0.1.0-f571c6688566` closed the private feedback loop in the UI:
  Settings lists the newest saved reports as compact, collapsed summaries and
  lets an operator copy the reviewed diagnostic bundle for a developer without
  exposing it by default. The deployed desktop and Android gates verified that
  the queue is visible without overflow or browser errors. The API update again
  preserved terminal-host PID `400662` and all three active sessions.
