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
  bounded suggestion result. The server rejects non-existent paths, symlinks,
  and filesystem roots; an existing folder outside configured workspace roots
  requires an explicit operator warning acknowledgement carried through the API
  and terminal-host request.
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
- Release `0.1.0-d8d469156fa6` added the first read-only Jira Cloud readiness
  adapter. The live endpoint reported the expected explicit `not_connected`
  state with no credentials configured. Desktop and Android Settings rendered
  that state without blocking local work, overflow, or browser errors, and the
  expanded gate verified it alongside every prior surface. Deployment retained
  terminal-host PID `400662` and all three active sessions.
- Release `0.1.0-6980cd5996b2` added the scannable `Awaiting you` worker state,
  derived only from unresolved durable decision requests while preserving
  `With you` during active operator engagement. Unit and persistence tests
  exercise both transitions; the deployed regression gate kept every desktop
  and Android surface bounded and error-free. Terminal-host PID `400662` and
  all three active sessions were again preserved.
- Release `0.1.0-c12380556b45` completed private screenshot retrieval for
  saved dogfood reports. Downloads require operator authentication, accept
  only opaque image names referenced by a retained report, revalidate the
  stored signature, disable caching and content sniffing, and force attachment
  download. The full Rust and frontend gates passed. The deployed desktop and
  Android pass kept all eight primary views within their viewports with no
  authenticated-page errors and verified feedback, Jira readiness, maintenance
  confirmation, and a valid SQLite backup. The API update preserved terminal-
  host PID `400662` and all three active sessions.
- Releases `0.1.0-74656e51c4dc` and `0.1.0-83b829270bc2` followed with a
  user-eye polish and recovery pass. Native checkbox/radio controls no longer
  inherit full-width text-input sizing, and Queen autonomy values remain fully
  readable in the two-column desktop Settings layout. The first deployed pass
  exposed client use of reserved WebSocket close codes during canonical
  recovery; the terminal now uses browser-valid application codes and unit
  tests pin both failure and fresh-snapshot paths. The strengthened live gate
  then selected Queen, Swarm Next, and Dogfood Clover in turn at desktop and
  Android sizes, verified each exact session became selected and connected,
  and completed every prior surface check without console or runtime errors.
  The same gate now closes the authenticated page, opens a fresh page in the
  browser context, and requires the control room to return without another
  operator-token prompt before any remaining checks run; both device profiles
  passed that PWA-reopen approximation.
- Settings coverage now scrolls to the lower backup and diagnostics region,
  captures it at both sizes, and inspects the scroll container plus every card
  for internal horizontal overflow. The live desktop workspace measured 1,154
  pixels exactly in both directions; mobile measured 412 pixels exactly, with
  zero overflowing cards in either profile.
- Release `0.1.0-36325e8497b2` refreshes the saved-feedback queue immediately
  after a report is retained, including when Settings was already mounted. All
  108 frontend tests and the production build passed. The deployed desktop and
  Android gates then exercised every primary surface and all three live workers,
  authenticated page reopen, guarded maintenance, feedback, Jira readiness,
  and the SQLite backup with no overflow or browser errors. The API-only update
  preserved terminal-host PID `400662`, all three running sessions, all five
  retained sessions, and the host's measured memory across the deployment.
- Release `0.1.0-cad55ededfc5` adds a sticky Settings section navigator so the
  growing daily-driver configuration surface no longer requires a blind long
  scroll. Crew, Presence, Queen, Appearance, System, Integrations, Backup, and
  Diagnostics remain keyboard reachable and become a bounded horizontal strip
  on mobile. The live gate used that navigator to reveal Diagnostics at both
  1,440 by 900 and 412 by 915, measured no document or card overflow, selected
  every live worker, and completed the full regression suite without browser
  errors. The API-only deployment again preserved terminal-host PID `400662`
  and all three running sessions.
- A continuous read-only soak then pinned both process identities for 600
  seconds: 20 samples kept API PID `959445` between 6.7 and 7.5 MiB and the
  terminal-host cgroup under PID `400662`, including all three Claude workers,
  between 1.19 and 1.21 GiB. All three sessions stayed live, retained history
  advanced only with terminal activity, and dropped history remained zero.
- Release `0.1.0-366849aa1202` completed the next mobile and Settings polish
  pass. Settings now exposes a visible 50-pixel section navigator with selected
  state, and mobile terminals offer a first-class image picker beside their
  optional key controls. Images use the existing private bounded attachment
  path and remain unsubmitted until the operator presses Enter. The deployed
  gate verified every primary surface and all three workers, the mobile picker,
  Settings selection and jump behavior, authenticated reopen, maintenance,
  feedback, Jira readiness, and backup with no browser errors or horizontal
  page/card overflow. Deployment preserved terminal-host PID `400662`, all
  three running sessions, and all five retained sessions.
- Release `0.1.0-67d625162b60` removes the false Settings count, leads worker
  creation with a repository-name search instead of a filesystem path, and
  makes both worker creation and waking sleeping workers reachable from quick
  navigation. Missing design-token aliases now have an automated completeness
  check, restoring the intended muted text, soft surfaces, danger color, and
  card radii throughout the UI. On mobile, the task queue now occupies the
  first screen while its complete creation form expands on demand; desktop
  keeps the form open by default.
- The deployed desktop and authentic Android-sized gates opened every primary
  surface, selected and connected all three exact live sessions, expanded the
  mobile task composer, verified repository-first worker creation and quick
  navigation, reopened the authenticated PWA context, exercised feedback,
  maintenance confirmation, Jira readiness, and downloaded a valid 356,352
  byte SQLite backup. Every document and Settings card remained bounded and no
  authenticated-page console or runtime error occurred. The API-only update
  replaced PID `969807` with `980976` while preserving terminal-host PID
  `400662`, the same three running session IDs, and all five retained sessions.
- A 30-minute owned-process browser soak then exercised Workers, Tasks, and
  Settings every minute against that unchanged deployed release. Sixty samples
  pinned Edge browser PID `42888`: browser private memory stayed between 61.8
  and 64.5 MiB and ended 0.8 MiB lower; its storage process stayed between 10.3
  and 10.4 MiB with zero growth; and the complete owned browser tree stayed
  between 323.9 and 343.1 MiB, ending 15.5 MiB higher with a 0.5 MiB/minute
  fitted slope. Renderer JavaScript stayed between 5.3 and 6.2 MiB, ending only
  0.2 MiB higher. DOM nodes remained exactly `440`, browser storage usage
  remained zero, and no page error was recorded. This directly covers the
  browser and storage processes implicated in the legacy incident; it is a
  bounded active sample, not a substitute for the Ring 2 multi-day soak.
- Release `0.1.0-0e2d095ab54b` completes the first global-search workflow.
  Quick navigation searches running or sleeping workers, open or completed
  work, and pending or resolved decisions; keyboard selection remains visible,
  Enter opens the exact result, completed containers expand automatically, and
  linked task/worker context is directly navigable. `Create task` opens the
  responsive composer and retains title focus in an actual Android-sized Edge
  context. The final deployed gate passed all eight primary surfaces, exact
  selection and connection of the same three live sessions, durable browser
  authentication, repository-first worker creation, feedback, maintenance,
  Jira readiness, restore guidance, and a valid 356,352-byte SQLite backup.
  Every desktop and mobile document/card remained bounded with no authenticated
  browser error. API PID `995654` now serves the release while terminal-host
  PID `400662` and all three worker session identities remain unchanged.
- Releases through `0.1.0-6624a47c17b6` completed the searchable completed-work
  view and fixed mobile quick-task focus at the lifecycle boundary where the
  collapsed composer actually mounts. The deployed desktop and Android gate
  found all seven completed tasks, expanded and focused an exact result, then
  opened `Create task` with the title field focused. All 119 frontend tests,
  the production build, and the full live gate passed while host PID `400662`
  and the same three session identities remained unchanged.
- Release `0.1.0-549271c15cf5` bounded automatic worker recovery. One automatic
  relaunch is allowed; a second early exit visibly blocks the worker until an
  operator Retry, while five stable minutes reset the circuit. The roster now
  names that action `Retry worker`. Unit tests cover repeated failure and the
  stable reset, and the API-only rollout preserved the live host and workers.
- Release `0.1.0-659c2584cd18` fixed a rolling-update authentication race rather
  than weakening the cookie. Live diagnostics proved the 30-day HttpOnly,
  Secure, SameSite=Strict cookie remained present and the session endpoint
  returned 204; a one-shot React bootstrap had landed inside the gateway
  handoff. Saved-session restoration now retries only network failures and
  HTTP 502/503/504 within a 15.75-second budget, never 401. A deliberate live
  API restart produced ten gateway errors, recovered the authenticated Settings
  workspace in 3.7 seconds, passed 17 memory samples and five navigation cycles,
  and retained terminal-host PID `400662`. The same release also restricts a
  worker's MCP task list to non-completed work bound to its current session,
  preventing historical tasks from polluting agent context while Queen keeps
  the complete Hive view.
- Release `0.1.0-4c2b31542f31` closes the adjacent terminal recovery gap found
  by the active browser soak. Terminal attachment previously exhausted its
  bounded retry budget before the replaceable API's supported update window
  ended. The retry window now matches authenticated bootstrap while remaining
  finite, and soak failures report the exact session, connection state, and
  content-free detail. A deliberate live API restart recovered Settings and
  the selected terminal in 7.1 seconds, retained the same sidecar PID and all
  three sessions, and passed 18 owned-browser memory samples. The complete
  post-deploy desktop and Android gate then reselected all three exact sessions
  as Connected, exercised all eight primary surfaces, found no document or
  Settings-card overflow, and downloaded a valid 356,352-byte Hive backup.
- Releases through `0.1.0-0c4ecb179c10` harden the remaining renderer lifecycle
  boundaries exposed by repeated real-browser navigation. Fit dimensions are
  finite safe integers, a transient reattach before layout no longer poisons a
  healthy connection, focus remains pending through canonical snapshot restore,
  and ordinary worker selection delivers focus after React mounts the selected
  terminal. The deployed desktop and Android gate selected Queen, Swarm Next,
  and Dogfood Clover as Connected, proved each terminal owned focus, exercised
  all eight primary surfaces without overflow, pasted an image into dogfood
  feedback, and preserved authentication across complete browser-process
  restarts at both viewport profiles. All 291 visible controls had accessible
  names and no authenticated-page browser error occurred.
- The final unchanged release then completed a paired one-hour acceptance run.
  The server observer collected 120 samples with all three sessions and
  terminal-host PID `400662` unchanged, zero dropped history, API RSS between
  5.6 and 7.9 MiB, and terminal-host cgroup memory between 1.54 and 1.65 GiB.
  Headless Edge collected 120 samples across 59 Workers/Tasks/Settings cycles:
  browser private memory grew 5.2 MiB at 0.06 MiB/minute, storage private memory
  grew 0.15 MiB, the complete owned process tree grew 28.6 MiB at 0.30
  MiB/minute, and renderer JavaScript ended 0.49 MiB lower. DOM nodes remained
  exactly `448`, browser storage remained zero, and every memory, reconnect,
  process-identity, and page-error gate passed.
- Releases through `0.1.0-43fec1ba2083` establish the first safe Jira intake
  loop. Queen can preview and explicitly import Jira issues, refresh already
  imported work, and transition linked issues without gaining access to Jira
  credentials. Automatic reconciliation refreshes only issues already owned by
  the Hive and never discovers or imports unseen Jira work. Imported task cards
  expose a keyboard-accessible link to the canonical Jira issue.
- Release `0.1.0-4a192bf7f7a4` removes the final bulk-import footgun: opening a
  Jira project review selects no issues, reports `Add 0 to this Hive`, and keeps
  import disabled until the operator explicitly chooses one or more tickets.
  The live desktop and Android-sized gate passed every primary surface with no
  page or card overflow, connected the same three exact worker sessions,
  exercised Jira review, feedback image paste, maintenance confirmation, and a
  valid 405,504-byte SQLite backup, and preserved authentication across complete
  browser-process restarts at both viewport profiles. The API-only deployment
  preserved terminal-host PID `400662` and all three running session IDs.
- A paired ten-minute post-Jira soak collected 40 browser and 40 server samples
  while actively cycling the UI. Edge browser private memory grew 1.5 MiB,
  storage private memory grew 96 KiB, the complete owned browser tree grew 11.8
  MiB, and renderer JavaScript grew 0.37 MiB; every fitted-slope and growth gate
  passed. The server retained all three sessions with zero dropped terminal
  history, API RSS between 6.6 and 7.4 MiB, and terminal-host cgroup memory
  within a 1 MiB band.
- The next Jira slice adds key/title/status/assignee filtering to large issue
  reviews and verifies in the live desktop and Android-sized browser that no
  ticket begins selected. Linked task transitions now commit a durable bounded
  Jira outbox record atomically with local state, survive network failure and
  API interruption, preserve pending local state against stale remote reads,
  and expose Updating, needs-attention, and explicit Retry states on the task
  card. Full workspace tests, strict clippy, all frontend tests, and the
  production build gate this slice before deployment.
- Release `0.1.0-33844fd0a466` adds an opt-in, same-port development reload
  without coupling worker lifetime to the replaceable API. The packaged release
  first replaced API PID `2416713` while preserving terminal-host PID `1528937`,
  two running sessions, and two retained sessions. A dedicated clean checkout
  was then configured on the dogfood host and one authenticated reload advanced
  the live app to `0.1.0-dev-33844fd0a466-20260814200458-2470159`. The reload
  completed with state `ready`; terminal-host PID and session counts remained
  unchanged. The first cold build peaked at 2.8 GiB while compiling Rust and
  then exited, so that transient build memory is not attributed to the daemon.
  Local desktop (1440x900) and Android (412x915) visual gates passed, and a live
  authenticated 1280x720 proof showed the development version, bounded Settings
  layout, preserved-worker diagnostics, and the reload control. All Chromium
  tabs were closed after proof.
- Releases through `0.1.0-dev-6b2faac33286` make the maintenance boundary
  explicit instead of presenting two similar update actions. The worker-engine
  path states that every loaded Claude or Codex process briefly stops, names an
  in-flight model turn or command as the real interruption risk, and preserves
  durable worker, conversation, task, ownership, and history state. The
  development reload path states that only API/web code is rebuilt and swapped,
  workers keep running, and a failed build leaves the current app active. The
  ambiguous `1 update ready` count is now `Update ready · restart required`;
  the separate affected-workers row owns the actual count. Live 1,440 by 900
  and 412 by 915 proofs found and corrected a cramped half-width desktop card,
  then measured exact page and card bounds with the comparison at two desktop
  columns and one mobile column. Three successive development reloads retained
  terminal-host PID `1528937` and the exact same Claude process set.
- The same pass shipped self-service Microsoft Outlook app registration in
  Settings. An operator sees the exact Web callback URL and required delegated
  permissions, submits tenant ID, client ID, and the one-time client-secret
  value over HTTPS, and receives only a secret-free configuration view. The
  host stores the registration privately and can reload it after an API restart;
  environment-managed registrations remain immutable from the browser. Live
  proof caught and fixed a clipped half-width form before handoff. The final
  1,440 by 900 and 412 by 915 layouts expose every field and action with zero
  document overflow, without submitting real Microsoft credentials.
