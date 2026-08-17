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

## Development reload provenance

Host-compatible releases and the development checkout now compare the exact
product source rather than assuming the package commit is the deployed source.
Reload is blocked when the configured checkout is older or unrelated, and a
failed-build marker applies only to the exact revision that failed. Packaging
tests, API tests, and the live System surface must prove that this fail-closed
state never restarts Claude, Codex, or the independently pinned worker engine.

## Deterministic stale-work attention

The next Queen-automation dogfood rule is deliberately observable rather than
interventionist. A loaded, Resting worker with durably owned Active work that
has not changed for 30 minutes produces one revision-bound coordination alert
only when the operator is not engaged with that session. The coordinator does
not type into the terminal or mutate local or Jira task state. Settings reports
the bounded count separately from mechanical wake actions, and Queen can read
the current evidence through a role-scoped tool before choosing to wait, steer,
or involve the operator.

Automated tests cover loaded/sleeping, Resting/non-Resting, engagement,
revision, idempotency, Queen fingerprinting, role-scoped tool access, and the
v62-to-v63 action-ledger migration. Rendered desktop and 412 by 915 acceptance
must confirm that the added coordinator metric remains readable before the
release is promoted.

## Resource-pressure admission for automatic starts

Automatic assigned-worker wakes are admitted before their durable action is
claimed. Advisory or critical machine/worker-engine pressure leaves the action
queued and observable, while an unavailable worker engine fails closed. The
operator can still wake a worker explicitly, and no running worker is stopped
or suspended by this rule.

Automated acceptance covers pressure precedence, partial evidence on hosts
without machine-wide sampling, fail-closed worker-engine availability, durable
queue retention, API visibility, and the operator-facing explanation. Rendered
desktop and 412 by 915 acceptance must confirm that the admission explanation
does not crowd the coordinator metrics before promotion.

## Exited-worker owned-work attention

An Active task no longer disappears from routine coordination when its worker
process exits. After the existing five-minute recovery window, the coordinator
records one revision- and process-bound observation only if no replacement
session or operator engagement exists. The observation enters Queen's bounded
review fingerprint and read-only coordination tool, but never restarts a
worker, injects a terminal, transitions a task, or writes to Jira. Recovery,
reassignment, task progress, or a lifecycle change makes it non-current.

Automated acceptance covers the grace boundary, idempotency, revision recheck,
replacement-session cancellation, schema 63-to-64 preservation, Queen
fingerprinting, API visibility, and the operator-facing combined work-surfaced
metric. Rendered desktop and 412 by 915 acceptance must confirm that the added
breakdown remains readable before promotion.

## Serialized automatic worker wakes

A normal resource sample admits only one deterministic worker start per
coordination pass. Additional Queen-originated Ready assignments remain queued
until a later pass obtains fresh API and worker-engine evidence. Manual starts
remain available, already running workers are never stopped, and an ambiguous
claimed start still becomes uncertain instead of replaying.

Automated acceptance covers two simultaneous sleeping-worker assignments,
single-claim ordering, durable remainder visibility, the public batch-limit
contract, and the operator explanation. Rendered desktop and 412 by 915
acceptance must confirm that the added safety explanation remains readable
before promotion.

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
- Release `0.1.0-531e76b00ac1` turns the advanced Apiary file exchange into an
  explicit three-step handoff. A personal Hive first shares only its signed
  public connection card, the Keeper verifies that exact Hive/operator and
  returns one bounded invitation file, and the invited operator reviews Keeper,
  policy, projects, and local readiness before preparing a join. The two file
  pickers now share one accessible drop-target component with a visible drag
  state. Playwright is a declared root development dependency, and the live
  acceptance gate now opens Apiary and captures dedicated evidence at 1,440 by
  900 and Android 412 by 915. Both profiles rendered exactly three steps, both
  controls, zero overflowing steps or drop targets, zero page/card overflow,
  and preserved authentication across complete browser restarts. The gate made
  no Jira selection or mutation and no Apiary state change. Deployment retained
  terminal-host PID `1528937` and the exact same 20 Claude processes.
- Releases `0.1.0-d2a8a508db9c` and `0.1.0-b99942eebc47` move the complete
  control-room snapshot behind one typed browser owner. Initial authentication,
  live-feed invalidation, manual refresh, worker lifecycle commands, and lock
  now replace or clear the same aggregate atomically; workspace choices and Jira
  links can no longer remain resident after the rest of the room is locked.
  Live proof found one mobile timing race where Worker engine briefly appeared
  unavailable before its request completed. The UI now says `Checking…`, and
  the gate waits for `Current` or `Update ready` before evaluating maintenance.
  The final authenticated run passed Needs you, Tasks, Workers, and Settings at
  exact 1,440 by 900 and 412 by 915 bounds, with 333 and 304 named controls,
  three Apiary invitation steps, and trusted-session survival across complete
  desktop and mobile browser restarts. Both deployments preserved terminal-host
  PID `1528937` and the exact same 20 Claude processes.
- Releases through `0.1.0-dev-e3dcea4ccf54` make development-reload detection
  content-aware and close a real mobile focus race. Repository-fixture tests
  prove that documentation and dogfood-script commits do not request an App/API
  reload, while Rust, web, packaging, or manifest changes do. The live safe
  reload advanced the App/API without changing terminal-host PID `2966127` or
  Queen session `01a004fc-68b6-7720-9a6e-0eebf2434db1`. The complete desktop
  and Android gate then passed all five main surfaces, Apiary member views,
  named-control checks, and authentication across browser-process restarts.
- Release `0.1.0-dev-aa6203df7a92` completes the first interactive Codex worker
  proof. A durable Codex profile started CLI `0.147.0`, completed a turn, slept,
  resumed in a different terminal process, and recalled the exact prior marker.
  Live Android testing then exposed Codex paste-burst protection: sending a
  composer payload and Enter as one burst left the prompt unsubmitted. The
  composer now sends explicit bracketed paste followed by a bounded, separate
  Enter frame. The rendered 412 by 915 flow produced an exact transformed
  response through both Codex Clover and Claude Dogfood Clover, returned both
  fixtures to Sleeping, and closed every Edge context after evidence capture.
- Releases through `0.1.0-dev-ebc3b1217d9b` give the Apiary handoff one
  component owner for its shared link, file-fallback, transport-status, and
  three-step guidance controls, then isolate the Keeper invitation workflow
  from the broader Apiary settings page. The complete frontend gate passed 206
  tests, strict TypeScript checking, and the production build. Live DOM proof
  on the Keeper Apiary showed the three invitation steps, verified-link entry,
  file fallback, promoted Jira project, ownership rollup, and collapse guard.
  The exact Edge acceptance gate passed at 1,440-pixel desktop and 412-pixel
  Android widths with zero document overflow, 346 and 318 named controls,
  three Apiary guide steps, member surfaces, and authentication preserved
  across complete browser restarts. The App/API-only deployment changed API
  PID to `3016004` while preserving terminal-host PID `2966127`.
- Release `0.1.0-dev-10537f325a0d` moves browser authentication, no-store
  requests, typed runtime failures, and bounded transient recovery behind one
  shared transport owner without changing the public API barrel. All 206
  frontend tests, strict TypeScript checking, and the production build passed.
  The source detector correctly requested an App/API reload for this product
  change, then returned to current after activation. Live acceptance passed all
  primary and member surfaces at 1,440-pixel desktop and 412-pixel Android
  widths with zero overflow, 346 and 314 named controls, three Apiary guide
  steps, and authentication preserved across complete browser restarts. API
  PID advanced to `3022514`; terminal-host PID remained `2966127`.
- Release `0.1.0-dev-95b0d085ea46` is the first complete API-domain split:
  presence vocabulary, reads, manual-mode changes, and device observations now
  share one focused module over the request transport while existing imports
  remain compatible. The gate passed 207 frontend tests, strict TypeScript,
  and the production build. Live desktop and Android acceptance again passed
  every primary and member surface with zero overflow, 346 and 318 named
  controls, three Apiary guide steps, and authentication across browser
  restarts. API PID advanced to `3027399`; terminal-host PID remained
  `2966127`.
- Release `0.1.0-dev-91b9ae4825be` moves durable worker discovery, repository
  choices, profile creation and editing, ordering, and start/stop commands into
  one worker-owned API module. Raw compatibility terminal-session commands stay
  outside that boundary. The public barrel remains compatible, and focused
  tests cover outside-root approval, encoded worker identities, lifecycle
  dimensions, ordering, settings, and roster behavior. All 208 frontend tests,
  strict TypeScript, and the production build passed. Exact live desktop and
  Android acceptance passed every primary and Apiary member surface with zero
  overflow, 346 and 318 named controls, three Apiary guide steps, worker
  selection/focus, and authentication across complete browser restarts. API
  PID advanced to `3035411`; terminal-host PID remained `2966127`.
- Release `0.1.0-dev-8ab18cc512ca` moves task vocabulary, bounded activity,
  ordering, creation, editing, lifecycle transitions, and assignment into one
  core-task API module. Jira synchronization and email intake remain owned by
  their integrations. Focused tests cover encoded task identities, activity
  limits, explicit unassignment, ordering, editing, and guarded transitions;
  all 209 frontend tests, strict TypeScript, and the production build passed.
  Exact live desktop and Android acceptance passed every primary and Apiary
  member surface with zero overflow, 346 and 318 named controls, task-board
  review, safe empty Jira selection, three Apiary steps, and authentication
  across browser restarts. API PID advanced to `3048085`; terminal-host PID
  remained `2966127`.
- Release `0.1.0-dev-f1f79ce299a9` moves Jira readiness, OAuth entry,
  project/workflow configuration, issue review, task links, comments, sync
  commands, retries, and reconciliation behind one integration-owned API
  boundary. Mocked transport tests cover every command shape and encoded
  identity without contacting Jira, and malformed rolling-update link payloads
  fail closed. All 211 frontend tests, strict TypeScript, and the production
  build passed. Live desktop and Android acceptance remained read-only: Jira
  discovery and issue review opened with no selected issue and performed no
  sync, assignment, comment, mapping, or workflow mutation. Every primary and
  Apiary member surface passed with zero overflow, 346 and 318 named controls,
  three Apiary guide steps, and authentication across browser restarts. API PID
  advanced to `3067260`; terminal-host PID remained `2966127`.
- Release `0.1.0-dev-b278aa2531e1` moves email configuration, bounded Inbox and
  message reads, attachment previews, multi-source task import, source links,
  deployment evidence, and the reviewed reply outbox behind one
  integration-owned API boundary. Mocked transport tests cover the read and
  command shapes without reading, importing, replying to, or otherwise
  mutating real mail. All 213 frontend tests, strict TypeScript, and the
  production build passed. Exact live Edge acceptance passed every primary and
  Apiary member surface at 1,440-pixel desktop and 412-pixel Android widths with
  zero overflow, 346 and 312 named controls, three Apiary guide steps, and
  authentication across complete browser restarts. The hidden browser harness
  closed after writing evidence. API PID advanced to `3075309`; terminal-host
  PID remained `2966127`, so the active worker engine and provider sessions were
  not restarted.
- Release `0.1.0-dev-70ee41308b41` gives personal-Hive joining one vertical
  browser owner for connection-card generation, invitation review/import,
  policy acknowledgement, project readiness, and durable prepared requests.
  The existing 16 Apiary interaction cases remained green inside the complete
  213-test frontend gate; strict TypeScript and the production build also
  passed. Exact live Edge acceptance passed Needs you, Tasks, Workers, Apiary,
  and Settings at 1,440-pixel desktop and 412-pixel Android widths with zero
  overflow, 346 and 318 named controls, and all three Apiary guide steps. The
  mocked member control room passed both sizes, authentication survived full
  browser restarts, and every hidden proof browser closed. API PID advanced to
  `3088579`; terminal-host PID remained `2966127`, preserving the active worker
  session while a separately pending worker-engine update stayed deferred.
- Release `0.1.0-dev-3eab4f698904` moves worker discovery, repository catalog
  and boundary validation, durable profile configuration/order, and wake/stop
  routes behind one API adapter without moving terminal process ownership.
  The clean Rust gate passed 331 workspace tests and Clippy with warnings
  denied; the focused API crate accounted for 109 of those tests. Live Edge
  acceptance passed every primary and Apiary member surface at 1,440-pixel
  desktop and 412-pixel Android widths with zero overflow, 346 and 314 named
  controls, all three Apiary guide steps, and authentication across browser
  restarts. API PID advanced to `3094914`; terminal-host PID remained
  `2966127`, preserving the active worker session.
- Release `0.1.0-dev-19f12ddb9d2b` moves core task list, creation, activity,
  ordering, editing, lifecycle transition, and assignment routes behind one
  HTTP adapter while application and persistence continue to own lifecycle
  rules and durability. The focused API gate passed 109 tests; the complete
  Rust workspace passed 331 tests and Clippy with warnings denied. All 213
  frontend tests, strict TypeScript, and the production build also passed.
  Live authenticated rendering exposed the exact release and complete task
  board. Read-only Edge acceptance then passed Needs you, Tasks, Workers,
  Apiary, Settings, and the Apiary member control room at 1,440-pixel desktop
  and 412-pixel Android widths with zero overflow, 346 and 318 named controls,
  all three Apiary guide steps, and authentication across full browser
  restarts. API PID advanced to `3100234`; terminal-host PID remained
  `2966127`, preserving the active worker session. Every proof tab and hidden
  browser process closed after the run.
- Release `0.1.0-dev-9d57d64a38e5` gives operator presence and device-scoped
  presentation preferences independent Rust HTTP owners. Manual At Hive/Away/
  Night Watch state and device observations remain application-owned;
  presentation persistence remains persistence-owned. Notification delivery
  and Queen autonomy policy were deliberately not coupled to either adapter.
  The complete gate passed 331 Rust tests, Clippy with warnings denied, 213
  frontend tests, strict TypeScript, and the production build. Read-only live
  Edge acceptance passed every primary and Apiary member surface at 1,440-pixel
  desktop and 412-pixel Android widths with zero overflow, 346 and 314 named
  controls, all three Apiary guide steps, and authentication across complete
  browser restarts. API PID advanced to `3105931`; terminal-host PID remained
  `2966127`, preserving the active worker session. The unauthenticated in-app
  proof tab and every authenticated hidden Edge process closed after use.
- Release `0.1.0-dev-371eb6297ac6` moves notification policy, subscription
  validation/lifecycle, test delivery, response shaping, and delivery
  scheduling beside the bounded Web Push sender. Existing endpoint allowlists,
  key validation, generic content-free payloads, and durable queue behavior
  remain unchanged. The complete gate passed 331 Rust tests, Clippy with
  warnings denied, 213 frontend tests, strict TypeScript, and the production
  build. Read-only live Edge acceptance passed every primary and Apiary member
  surface at 1,440-pixel desktop and 412-pixel Android widths with zero
  overflow, 346 and 318 named controls, all three Apiary guide steps, and
  authentication across full browser restarts. API PID advanced to `3110299`;
  terminal-host PID remained `2966127`, preserving the active worker session.
  Every proof browser process closed after the run.
- Release `0.1.0-dev-43e82f295732` moves the Queen's At Hive, Away, and Night
  Watch autonomy ceilings behind a focused orchestration HTTP owner while
  persistence remains authoritative and actual conductor execution remains
  gated by repository/environment policy. The code release passed 331 Rust
  tests, Clippy with warnings denied, 213 frontend tests, strict TypeScript,
  and the production build before activation. Read-only live Edge acceptance
  passed every primary and Apiary member surface at 1,440-pixel desktop and
  412-pixel Android widths with zero overflow, 346 and 315 named controls, all
  three Apiary guide steps, and authentication across complete browser
  restarts. API PID advanced to `3118527`; terminal-host PID remained
  `2966127`, preserving the active worker session. Every proof browser process
  closed after the run.
- Release `0.1.0-dev-45ef43e886c4` gives operator-decision listing, guarded
  resolution, outcome delivery state, and bounded retry routes one focused
  Rust HTTP owner while application and persistence services retain authority
  for decision policy and durability. The complete gate passed 331 Rust tests,
  Clippy with warnings denied, 213 frontend tests, strict TypeScript, and the
  production build. Exact live Edge acceptance passed Needs you, Tasks,
  Workers, Apiary, Settings, and the Apiary member control room at 1,440-pixel
  desktop and 412-pixel Android widths with zero overflow, 346 and 318 named
  controls, all three Apiary guide steps, and authentication across complete
  browser restarts. API PID advanced to `3139756`; terminal-host PID remained
  `2966127`, preserving the active worker session. Every proof browser process
  closed after the run.
- Release `0.1.0-dev-1a2e19078d34` moves content-free control-room event append,
  bounded retention, resumable reads, and stale-cursor reset into one focused
  persistence aggregate without changing its serialized contract. The full
  release gate passed 331 Rust tests, Clippy with warnings denied, 213 frontend
  tests, strict TypeScript, and the production build. Live Edge acceptance
  passed Needs you, Tasks, Workers, Apiary, Settings, and the Apiary member
  control room at 1,440-pixel desktop and 412-pixel Android widths with zero
  overflow, 346 and 314 named controls, all three Apiary guide steps, and
  authentication across complete browser restarts. API PID advanced to
  `3152636`; terminal-host PID remained `2966127`, preserving the active worker
  session. Every proof browser process closed after the run.
- Release `0.1.0-dev-c180ce35e4d3` gives authenticated privacy-safe dogfood
  report listing/creation and private attachment upload/download one focused
  Rust HTTP owner while the bounded attachment store retains media validation
  and private storage. The full release gate passed 331 Rust tests, Clippy with
  warnings denied, 213 frontend tests, strict TypeScript, and the production
  build. Exact live Edge acceptance included feedback image paste, private-save
  availability, and the saved-report queue while passing every primary and
  Apiary member surface at 1,440-pixel desktop and 412-pixel Android widths
  with zero overflow, 346 and 318 named controls, all three Apiary guide steps,
  and authentication across complete browser restarts. API PID advanced to
  `3167607`; terminal-host PID remained `2966127`, preserving the active worker
  session. Every proof browser process closed after the run.
- Release `0.1.0-dev-321e7eaa77a3` gives the authenticated, no-store consistent
  SQLite export one focused Rust HTTP owner while online backup creation and
  integrity remain persistence responsibilities. The full release gate passed
  331 Rust tests, Clippy with warnings denied, 213 frontend tests, strict
  TypeScript, and the production build. Live Edge acceptance verified private
  backup availability while passing every primary and Apiary member surface at
  1,440-pixel desktop and 412-pixel Android widths with zero overflow, 346 and
  318 named controls, all three Apiary guide steps, and authentication across
  complete browser restarts. API PID advanced to `3174241`; terminal-host PID
  remained `2966127`, preserving the active worker session. Every proof browser
  process closed after the run.
- Release `0.1.0-dev-9e01dede1352` gives browser-session creation, lookup,
  revocation, constant-time bearer and cookie verification, secure-request
  detection, and the HttpOnly SameSite cookie one focused Rust HTTP owner. The
  full release gate passed 331 Rust tests, Clippy with warnings denied, 213
  frontend tests, strict TypeScript, and the production build. Exact live Edge
  acceptance passed every primary and Apiary member surface at 1,440-pixel
  desktop and 412-pixel Android widths with zero overflow, 346 and 318 named
  controls, all three Apiary guide steps, and authentication preserved across
  complete browser restarts. API PID advanced to `3199255`; terminal-host PID
  remained `2966127`, preserving the active worker session. Every proof browser
  process closed after the run.
- Release `0.1.0-dev-5e11567a2a3f` gives the authenticated control-room event
  feed one focused Rust HTTP owner for cursor normalization, bounded long
  polling, resumable page delivery, stale-cursor reset, and no-store response
  policy. Its focused private/resumable/content-free test and the full release
  gate passed: 331 Rust tests, warnings-denied Clippy, 213 frontend tests,
  strict TypeScript, and the production build. Exact live Edge acceptance
  passed every primary and Apiary member surface at 1,440-pixel desktop and
  412-pixel Android widths with zero overflow, 345 and 314 named controls, all
  three Apiary guide steps, and authentication preserved across complete
  browser restarts. API PID advanced to `3204299`; terminal-host PID remained
  `2966127`, preserving the active worker session. Every proof browser process
  closed after the run.
- Release `0.1.0-dev-9acac593840c` gives read-only runtime limits, API and
  terminal-host process resources, Linux machine pressure, terminal-host
  status, and content-aware development source/reload state one focused Rust
  HTTP owner. Focused tests covered authentication, published bounds, resource
  thresholds, source-version parsing, and product-only change detection. The
  full release gate passed 331 Rust tests, warnings-denied Clippy, 213 frontend
  tests, strict TypeScript, and the production build. Exact live Edge
  acceptance exercised Settings diagnostics while passing every primary and
  Apiary member surface at 1,440-pixel desktop and 412-pixel Android widths
  with zero overflow, 346 and 314 named controls, all three Apiary guide steps,
  and authentication preserved across complete browser restarts. API PID
  advanced to `3222930`; terminal-host PID remained `2966127`, preserving the
  active worker session. Every proof browser process closed after the run.
- Release `0.1.0-dev-40c4e28ed53f` gives development-reload requests and
  explicitly disruptive worker-engine maintenance one focused Rust command
  owner, separate from read-only runtime observation. Focused tests proved
  authentication, fail-closed unavailable installs, content-free reload
  requests, no worker restart for a matching engine, and the managed stop,
  update, cleanup, and configured-worker revival path. The full release gate
  passed 331 Rust tests, warnings-denied Clippy, 213 frontend tests, strict
  TypeScript, and the production build. Exact live Edge acceptance exercised
  the maintenance confirmation surface without applying a worker update and
  passed every primary and Apiary member surface at 1,440-pixel desktop and
  412-pixel Android widths with zero overflow, 345 and 315 named controls, all
  three Apiary guide steps, and authentication preserved across complete
  browser restarts. API PID advanced to `3235842`; terminal-host PID remained
  `2966127`, preserving the active worker session. Every proof browser process
  closed after the run.
- Release `0.1.0-dev-d44b2cfe0be1` gives bounded concurrent provider snapshot
  observation, loaded-state refresh, content-free runtime event publication,
  and provider capability fallback one focused Rust owner. Focused tests proved
  loaded-idle, active, and unloaded worker distinctions plus private capability
  reads from older worker engines. The full release gate passed 331 Rust tests,
  warnings-denied Clippy, 213 frontend tests, strict TypeScript, and the
  production build. Exact live Edge acceptance exercised the real worker list
  and passed every primary and Apiary member surface at 1,440-pixel desktop and
  412-pixel Android widths with zero overflow, 346 and 318 named controls, all
  three Apiary guide steps, and authentication preserved across complete
  browser restarts. API PID advanced to `3245450`; terminal-host PID remained
  `2966127`, preserving the active worker session. Every proof browser process
  closed after the run.
- Release `0.1.0-dev-ad86d67c2e22` gives filtered live sessions, privacy-safe
  history diagnostics, retained-session discovery, validated history cursors,
  and resumable durable history reads one focused Rust HTTP owner. Focused
  tests proved authorization and content-free diagnostics, durable pagination
  after reopening storage, and exclusion of completed provider processes from
  the live-session list. The full release gate passed 331 Rust tests,
  warnings-denied Clippy, 213 frontend tests, strict TypeScript, and the
  production build. Exact live Edge acceptance passed every primary and
  Apiary member surface at 1,440-pixel desktop and 412-pixel Android widths
  with zero overflow, 346 and 318 named controls, all three Apiary guide steps,
  and authentication preserved across complete browser restarts. API PID
  advanced to `3253352`; terminal-host PID remained `2966127`, preserving the
  active worker session. Every proof browser process closed after the run.
- Release `0.1.0-dev-9c671acdf990` gives authenticated terminal start, bounded
  output reads, input writes, validated resizes, and assignment-releasing stop
  commands one focused Rust HTTP owner without moving worker lifecycle or
  WebSocket streaming across their established boundaries. Focused tests
  proved fail-closed authentication, invalid-token rejection, host-owned
  session survival across API recreation, and real WebSocket replay and
  control. The full release gate passed 331 Rust tests, warnings-denied Clippy,
  213 frontend tests, strict TypeScript, and the production build. Exact live
  Edge acceptance inspected the real terminal at 1,440-pixel desktop and
  412-pixel Android widths while passing every primary and Apiary member
  surface with zero overflow, 346 and 318 named controls, all three Apiary
  guide steps, and authentication preserved across complete browser restarts.
  API PID advanced to `3268019`; terminal-host PID remained `2966127`,
  preserving the active worker session. Every proof browser process closed
  after the run.
- Release `0.1.0-dev-7c9df199ba91` gives one-time attachment grants, bounded
  WebSocket upgrade, device-owned engagement release, and authenticated private
  terminal image upload one focused Rust HTTP owner while byte streaming stays
  in the terminal socket engine and media validation stays in the attachment
  store. Focused tests proved attachment authorization, real WebSocket replay
  and control, fail-closed terminal routes, and quiet-Queen coordination after
  engagement. The full release gate passed 331 Rust tests, warnings-denied
  Clippy, 213 frontend tests, strict TypeScript, and the production build.
  Exact live Edge acceptance inspected the connected terminal and Android image
  picker while passing every primary and Apiary member surface at 1,440-pixel
  desktop and 412-pixel Android widths with zero overflow, 346 and 314 named
  controls, all three Apiary guide steps, and authentication preserved across
  complete browser restarts. API PID advanced to `3282775`; terminal-host PID
  remained `2966127`, preserving the active worker session. Every proof browser
  process closed after the run.
- Release `0.1.0-dev-6881ecbe20d4` gives durable worker startup, provider
  conversation selection, outside-root policy, host-session binding rollback,
  and stale-session reconciliation one focused Rust runtime owner. Focused
  tests proved Queen reattachment without duplication, recovery-circuit reset
  after stability, bounded repeated recovery, and assignment to sleeping
  workers. The full release gate passed 331 Rust tests, warnings-denied Clippy,
  213 frontend tests, strict TypeScript, and the production build. Exact live
  Edge acceptance inspected the preserved Queen terminal while passing every
  primary and Apiary member surface at 1,440-pixel desktop and 412-pixel
  Android widths with zero overflow, 345 and 318 named controls, all three
  Apiary guide steps, and authentication preserved across complete browser
  restarts. API PID advanced to `3294941`; terminal-host PID remained
  `2966127`, preserving the active worker session. Every proof browser process
  closed after the run.
- Release `0.1.0-dev-960dc130d9fb` gives host availability, typed IPC failure
  handling, authenticated terminal commands, no-store reads, and content-free
  session invalidation one shared terminal-host client boundary used by
  runtime, history, attachment, control, maintenance, worker recovery, and
  provider observation. The full Rust gate passed 331 tests and
  warnings-denied Clippy. Exact live Edge acceptance passed every primary and
  Apiary member surface at 1,440-pixel desktop and 412-pixel Android widths
  with zero overflow, 345 and 318 named controls, all three Apiary guide steps,
  and authentication preserved across complete browser restarts. API PID
  advanced to `3320473`; terminal-host PID remained `2966127`, preserving the
  active worker session. Every proof browser process closed after the run.
- Release `0.1.0-dev-99fa13fdfde1` replaces the manual Apiary file exchange
  with one bounded Keeper link and a durable member-owned outbound poll. The
  private capability stays server-side, exact Hive identity requires explicit
  Keeper approval, and the approved signed invitation imports automatically on
  the member's next poll. A real loopback HTTP integration test spans both Hive
  APIs and proves that the member initiates every connection. Jira remains a
  direct per-Hive integration; Keeper polling carries federation state and is
  the future Native Apiary task channel. The release gate passed 337 Rust tests,
  warnings-denied Clippy, 213 frontend tests, strict TypeScript, and the
  production build. The updated live Edge gate passed every primary and Apiary
  member surface at 1,440-pixel desktop and 412-pixel Android widths with zero
  overflow, 344 and 316 named controls, both transport-boundary explanations,
  and authentication preserved across complete browser restarts. API PID
  advanced to `3367448`; terminal-host PID remained `2966127`, preserving the
  active worker session. Every proof browser context closed after the run.
- Release `0.1.0-dev-38e9f44c8066` completes the member-owned signed join and
  adds automatic post-membership Keeper catalog polling with durable bounded
  backoff and operator-meaningful halt states. Jira reconciliation remains a
  separate direct per-Hive loop; no Jira issue content or credential traverses
  Keeper. The release gate passed 338 Rust tests, warnings-denied Clippy, 213
  frontend tests, strict TypeScript, and the production build. The first live
  pass also exposed and fixed a completed-task quick-navigation focus race.
  The final live browser gate passed every primary and Member Apiary surface at
  1,440-pixel desktop and 412-pixel Android widths with zero overflow, 343 and
  315 named controls, both transport-boundary explanations, and authentication
  preserved across complete browser restarts. API PID advanced to `3423836`;
  terminal-host PID remained `2966127`, preserving the active worker session.
  Every isolated proof browser closed after the run.
- The governed Apiary-task command checkpoint adds a durable Member outbox,
  revision-checked Keeper claims and transitions, retry-stable receipts, and
  ordered event projection without routing Jira through Keeper. Real in-memory
  Keeper/Member tests covered offline queueing, authenticated application,
  exact retry, stale conflict, and projection. The full gate passed 342 Rust
  tests, warnings-denied Clippy, 213 frontend tests, strict TypeScript, and the
  production build. An isolated real Member runtime on port 8767 showed three
  Keeper tasks at both 1,440 by 900 and 412 by 915. Claiming while Keeper was
  unreachable changed one task to `Queued for Keeper`, incremented the durable
  pending count, survived reload, and kept horizontal overflow at zero. The
  browser console had no warnings or errors, and the proof tab and temporary
  runtime processes were closed afterward.
- Release `0.1.0-dev-0056e623714d` gives Keeper a first-class shared-task
  creation flow over the existing canonical Apiary task command. The form
  captures an outcome, optional context, and priority while deliberately
  omitting private Member workers and repositories. The frontend gate passed
  218 tests, strict TypeScript, and the production build. Live proof on the
  real Keeper Apiary showed the shared-work panel expanding to 845 pixels at
  desktop width, a single-column Android form with 317-pixel controls at 412
  by 915, disabled empty submission, and zero horizontal overflow. The API
  advanced to the new release while terminal-host PID `2966127` remained
  unchanged, preserving the active Queen session. Every proof browser tab was
  closed afterward.
- Release `0.1.0-dev-e365562241b5` gives Keeper Queen private
  `swarm_list_apiary_tasks` and `swarm_create_apiary_task` tools while ordinary
  workers neither discover nor gain authority by directly naming them. The
  tools list and create only Keeper-canonical Swarm work; they do not expose or
  target a remote Hive's private worker, repository, terminal, or Jira issue
  content. The full Rust gate passed 343 tests plus warnings-denied Clippy, and
  all six focused MCP bridge tests passed after the final lint cleanup. Live
  read-only MCP discovery against the preserved Queen credential returned both
  new tool names. API PID advanced to `3537204`; terminal-host PID remained
  `2966127`, preserving the active Queen session.
- Release `0.1.0-dev-4dcc2f52e794` extends the Queen bridge with governed
  Member-side `swarm_claim_apiary_task` and `swarm_transition_apiary_task`
  commands. Mutations enter the Member's durable outbound queue and retain the
  existing revision, retry, and conflict rules; ordinary workers still cannot
  discover or invoke Apiary authority. The full Rust gate passed 343 tests and
  warnings-denied Clippy after clearing one corrupted incremental compiler
  cache. Live read-only MCP discovery against the preserved Queen credential
  returned list, create, claim, and transition. API PID advanced to `3544183`;
  terminal-host PID remained `2966127`, preserving the active Queen session.
- Releases `0.1.0-3ed84cd821f0` and `0.1.0-6ff646bfa62b` add durable worker
  management for daily dogfooding. Sleeping workers can change their default
  coding provider, keep an operator-reviewed Queen-routing description, and be
  archived without deleting repositories or retained history. Removal refuses
  Queen, running workers, and workers with open assignments. A private local
  draft reads only bounded top-level README and project metadata files; it
  never calls a provider and remains unsaved until the operator reviews and
  saves it. The Queen roster tool includes the reviewed description, with an
  explicit MCP contract test preventing that routing context from being
  dropped. The full implementation gate passed 347 Rust tests,
  warnings-denied Clippy, 220 frontend tests, strict TypeScript, and the
  production build. Live proof exercised the draft on the sleeping disposable
  Codex worker and cancelled without changing the profile. Responsive image
  capture was not recorded because the in-app Chromium target closed during
  capture; all proof tabs were closed immediately. API PID advanced to
  `3583500`; terminal-host PID remained `2966127`, preserving Queen.
- Release `0.1.0-c7ad3db2d390` completes the local-first routing-description
  flow by generating the same bounded private draft when a repository worker
  is first added. The operator can review, revise, or discard it before the
  description becomes Queen-visible; creation still makes no provider call.
  The implementation gate passed 349 Rust tests, warnings-denied Clippy, 220
  frontend tests, strict TypeScript, and the production build. A fresh live
  acceptance run exercised every primary and Apiary surface at 1,440 by 900
  and 412 by 915 with zero horizontal overflow, 344 and 312 named controls,
  safe Jira and email review, and authentication preserved across complete
  browser restarts. Both mocked Member Apiary surfaces also passed. API PID
  advanced to `3587600`; terminal-host PID remained `2966127`, preserving the
  active Queen session. Every headless browser and context closed after the
  run.
- Release `0.1.0-dev-b605731430d5` promotes the exact legacy Project Root into
  managed Scout without replacing her durable identity, and adds an optional
  bounded, tool-free Claude review of the local routing-description packet.
  Test commit `3460c77` extends the live acceptance contract to require Scout
  directly after Queen, prevent managed rename/removal/reordering, expose the
  sleeping provider and description controls, and explain exactly where a
  receiving personal Hive pastes a Keeper invitation link. All 221 frontend
  tests and strict TypeScript passed. The live dogfood gate passed every
  primary and mocked Member surface at 1,440 by 900 and 412 by 915 with zero
  horizontal overflow, 340 and 308 accessible controls, both Apiary transport
  explanations, and authentication preserved across complete browser
  restarts. API PID `3641269` remained healthy; terminal-host PID `2966127`
  remained unchanged with the active Queen session preserved. Every proof
  browser and context closed after the run.
- Release `0.1.0-dev-dea70797b2b3` gives a joined Member a separate,
  credential-bound projection of only her own Keeper-granted Steward scope.
  Grant replacement, exact retry, identity tampering, and explicit revocation
  are covered at persistence; transport and API tests prove the node-bound,
  content-free response; unsupported or malformed snapshots halt federation
  reconciliation instead of retaining stale authority. The Member control
  room now presents **My Stewardship** separately from personal Hive work and
  names the managed Hives and capabilities without implying that staged remote
  commands already exist. The full Rust workspace tests, 221 frontend tests,
  warnings-denied Clippy, formatting, strict TypeScript, and production build
  passed. A verified 1,085,440-byte SQLite backup preceded schema 52; the live
  database then reported `integrity_check=ok`. Isolated live Member proof passed
  at 1,440 by 900 and 412 by 915 with zero horizontal overflow, 11 and 10 named
  controls, and a final image review corrected mobile capability truncation.
  API PID advanced to `3682916`; terminal-host PID `2966127` remained unchanged,
  preserving the active Queen session. Every proof browser and context closed.
- Test revision `792ba1f` closes the remaining worker-management acceptance
  gap without mutating a live profile. The authenticated browser gate now opens
  an ordinary sleeping worker, requires the repository identity, editable
  Queen-routing description, bounded local and Claude drafting controls,
  default-provider selector, and guarded removal action, then enters and backs
  out of the explicit removal confirmation. The same run confirms that the
  Keeper screen tells the receiving operator to paste the private link in her
  own personal Hive under **Settings -> Apiary -> Join a Keeper's Apiary**.
  Desktop at 1,440 by 900 and Android-size mobile at 412 by 915 passed every
  primary and mocked Member surface with zero horizontal overflow, 340 and 308
  named controls, and authentication preserved across complete browser
  restarts. Jira and email intake were deliberately skipped for this proof so
  changing live source data could not block or mutate the maintenance checks.
  API PID remained `3682916`; terminal-host PID `2966127` remained unchanged.
  Every proof browser and context closed after the run.
- Release `0.1.0-dev-8b6ef0068439` makes the cross-installation handoff explicit
  from both sides. A Keeper-generated link now repeats its exact destination;
  the receiving personal Hive presents a compact three-step journey: paste the
  private link in that Hive, wait for the Keeper to approve its signed public
  identity, then review policy and Jira readiness before joining. Opening the
  Keeper-hosted URL is explicitly distinguished from ingesting it into the
  receiving Hive. All 221 frontend tests, strict TypeScript, and the production
  build passed. Live acceptance covered every primary surface plus isolated
  Keeper, Member, and personal-Hive views at 1,440 by 900 and Android-size 412
  by 915. All surfaces had zero horizontal overflow; the personal-Hive view
  exposed 88 named controls at both sizes; authentication survived full browser
  restarts. API PID advanced to `3708322`; terminal-host PID `2966127` and its
  worker-engine build remained unchanged. Every proof browser and context
  closed after the run.
- Release `0.1.0-dev-7017a2ad1fc7` lets a Keeper cancel an invitation that has
  not yet delivered its signed membership. Cancellation and delivery compete
  in one atomic persistence update: an open, identity-presented, or approved
  private link can be revoked, while a link whose invitation was already
  issued is deliberately too late to cancel. The Keeper UI requires an
  explicit confirmation and offers **Keep link** before making the private
  link unusable. Full Rust workspace tests, warnings-denied Clippy, formatting,
  all 221 frontend tests, strict TypeScript, and the production build passed.
  Live proof opened and backed out of that guard without cancelling the real
  pending invitation. Every primary, Member, and personal-Hive surface passed
  at 1,440 by 900 and Android-size 412 by 915 with zero horizontal overflow;
  image review confirmed the guard is readable and unambiguous at both sizes.
  API PID advanced to `3725960`; terminal-host PID `2966127` remained unchanged,
  preserving all active workers. Every proof browser and context closed.
- Release `0.1.0-dev-110c81bd0fba` completes the receiving personal-Hive side
  of that lifecycle. A revoked or expired private link is labelled in plain
  language, stops polling the Keeper, and can be removed only through an
  explicit local confirmation; an operator may also deliberately stop waiting
  on a still-live link. Removing it changes no Keeper or Apiary state and
  explains that the private link must be pasted again to reconnect. The full
  Rust workspace, 222 frontend tests, warnings-denied Clippy, formatting,
  strict TypeScript, and production build passed. Isolated personal-Hive proof
  rendered the cancelled state and backed out of its removal guard at 1,440 by
  900 and Android-size 412 by 915. Every primary, Member, and personal-Hive
  surface remained free of horizontal overflow, and browser authentication
  survived complete restarts. API PID advanced to `3733677`; terminal-host PID
  `2966127` remained unchanged. Every proof browser and context closed.
- Release `0.1.0-dev-c293119af824` adds retry-safe Member departure without sacrificing private
  Hive work. The flow names exactly what stays local, blocks departure while
  shared Jira claims, Apiary tasks, Stewardships, or queued shared mutations
  remain, and requires the Apiary name before leaving. If the Keeper response
  is lost, the Member remains safely paused and retries the same signed request;
  no partial departure is presented as complete. The full Rust workspace,
  225 frontend tests, warnings-denied Clippy, formatting, strict TypeScript,
  and the production build passed. The real component was visually proofed in
  an isolated non-mutating Member surface at 1,440 by 900 and Android-size 412
  by 915. Normal, confirmation, and paused-recovery states had zero horizontal
  overflow and kept all safeguards readable. An earlier verified schema-52
  backup remained available and a 1,110,016-byte schema-53 snapshot passed
  `quick_check=ok` after activation. Live desktop and mobile smoke tests showed
  the new release with zero horizontal overflow. API PID advanced to `3768640`;
  terminal-host PID `2966127` remained unchanged, preserving active workers.
  Every proof tab closed immediately after the run.
- Release `0.1.0-dev-4ace1b6f2a32` removes operator memory from migration
  safety. Every API or protocol update now creates and verifies a consistent
  online Hive database backup before switching releases, keeps the newest ten,
  and restores the exact pre-update database if activation fails after a schema
  change. The isolated package lifecycle proves both compatible-API and full
  protocol failure paths restore database contents while leaving active worker
  handling unchanged. The live compatible update automatically created a
  1,110,016-byte schema-53 pre-update backup; both it and the live database
  passed `quick_check=ok`. API PID advanced to `3775469`; terminal-host PID
  `2966127` remained unchanged.
- Release `0.1.0-dev-24d64f4626a7` completes confirmed Jira claim handoff as
  one operator-facing, Keeper-authoritative workflow. A source Hive offers one
  destination; the destination explicitly accepts or declines, records its
  reconciliation intent before assigning through its own Jira identity, and
  becomes the durable home only after Keeper confirmation. Exact retries,
  cancellation before acceptance, authentication rejection, and restart-safe
  recovery are covered across persistence, transport, local API, and React
  tests. Full Rust workspace tests, warnings-denied Clippy, all 225 frontend
  tests, strict TypeScript, and the production build passed. The live API moved
  to schema 55 with `quick_check=ok`; API PID advanced to `3810111` while
  terminal-host PID `2966127` remained unchanged. The authenticated browser
  gate populated incoming and outgoing handoff controls with route-local public
  fixtures and passed every live primary, Member, and personal-Hive surface at
  1,440 by 900 and Android-size 412 by 915 with zero horizontal overflow.
  Image review confirmed readable stacked mobile actions and compact desktop
  actions. No Jira or live Apiary mutation occurred, authentication survived
  complete browser restarts, and every proof browser closed after the run.
- Release `0.1.0-dev-cccac50b1740` lets Keeper route a new Swarm-generated
  shared task directly to one active Member Hive while preserving the private
  worker boundary. The destination is selected from public Hive identities;
  the receiving Queen still chooses her repository worker. Unknown or departed
  Hives fail closed, ordinary workers cannot discover or invoke the authority,
  and Keeper Queen can list only public Apiary Hive/operator identities before
  routing. The full gate passed 378 Rust tests, warnings-denied Clippy, all 226
  frontend tests, strict TypeScript, the production build, and dogfood harness
  tests. The isolated live browser gate populated a Clover Hive destination
  and passed Keeper routing, Member, personal-Hive, and every primary surface
  at 1,440 by 900 and Android-size 412 by 915 with zero horizontal overflow.
  Image review confirmed a compact four-field desktop form and a readable
  single-column mobile form without a private worker selector. Authentication
  survived complete browser restarts. The live database remained schema 55
  with `quick_check=ok`; API PID advanced to `3827684` while terminal-host PID
  `2966127` remained unchanged. Every isolated proof browser closed after the
  run, and no Jira or live Apiary mutation occurred.
- Release `0.1.0-dev-5a7d2dc44f7b` turns a Keeper invitation into a guided
  handoff that a personal-Hive operator can use without understanding URL
  fragments or copying secrets between settings fields. The recipient can
  retarget the exact private fragment to her normal HTTPS Hive address, or use
  the already-open personal Hive and review the Keeper and Apiary before any
  membership begins. The capability remains in the browser fragment and
  transient module memory only: it is never relayed, written to browser
  storage, or placed in navigation history at the destination. Manual paste
  remains available as a recovery path. All 231 frontend tests, strict
  TypeScript, the production build, and the responsive browser gate passed.
  Image review confirmed a calm, legible handoff at 1,440 by 900 and Android-
  size 412 by 915; every primary, Keeper-routing, Member, and personal-Hive
  surface retained zero horizontal overflow, and authentication survived a
  complete browser restart. The live database remains schema 55 with
  `quick_check=ok`; API PID advanced to `3836749` while terminal-host PID
  `2966127` remained unchanged. No Jira or live Apiary mutation occurred, and
  every proof browser closed after the run.
- Release `0.1.0-dev-9ffaf493050d` completes the private side of Keeper task
  routing. After the task reaches its Member Hive, that Hive's Queen can choose
  one reviewed repository worker and materialize one durable local task.
  Exact retries reuse the same task and worker; the Keeper receives no worker,
  repository, terminal, provider-session, or execution evidence. The Queen-only
  agent tool follows the same boundary, while ordinary workers cannot discover
  it. The gate passed 379 Rust tests, warnings-denied Clippy, formatting, all
  233 frontend tests, strict TypeScript, the production build, and the dogfood
  harness. Isolated and deployed fixture proof rendered the private-worker route
  at 1,440 by 900 and Android-size 412 by 915. The first desktop capture exposed
  a clipped two-column action; moving Keeper tasks to the full dashboard width
  resolved it before release. Every primary, Keeper, Member, and personal-Hive
  surface passed with zero horizontal overflow, and authentication survived
  complete browser restarts. The package created a verified 1,155,072-byte
  pre-update backup and migrated the live database to schema 56 with
  `quick_check=ok`. API PID advanced to `3855825`; terminal-host PID `2966127`
  remained unchanged. No Jira or live Apiary mutation occurred, and every proof
  browser closed after the run.
- Release `0.1.0-dev-ddab444c3e7a` makes the linked local task the sole worker
  lifecycle authority while preserving Keeper's canonical shared record. Each
  local transition atomically updates a durable desired shared state. Member
  reconciliation stages only the next legal command, waits until its applied
  revision returns in Keeper's task feed, and then advances again. Focused proof
  moved a local task from Ready through Active to Review while Keeper lagged;
  only Active was queued first, exact preparation retries produced no duplicate,
  Review waited for revision 2, and both records converged at revision 3. Direct
  shared transitions fail closed after materialization, conflicts stop the chain,
  and private worker or repository data still never enters Keeper. The full gate
  passed 380 Rust tests, warnings-denied Clippy, formatting, all 234 frontend
  tests, strict TypeScript, the production build, and the dogfood harness.
  Desktop and Android-size proof rendered both “Send to worker” and “Keeper
  ready · syncing to active” states with zero horizontal overflow. Every primary,
  Keeper, Member, and personal-Hive surface passed live, and authentication
  survived complete browser restarts. The package created a verified
  1,171,456-byte pre-update backup and migrated the database to schema 57 with
  `quick_check=ok`. API PID advanced to `3867547`; terminal-host PID `2966127`
  remained unchanged. No Jira or live Apiary mutation occurred, and every proof
  browser closed after the run.
- Release `0.1.0-dev-69bb4f24dfb0` adds the first guarded Steward action. A
  Member with synchronized **Assign** authority can journal one bounded task
  command for a managed Hive; Keeper authenticates the member node and operator,
  rechecks the current scope and target membership in the creation transaction,
  and returns one retry-stable applied or rejected receipt. The target Hive still
  chooses its private worker and repository. The full gate passed 382 Rust tests,
  warnings-denied Clippy, formatting, all 235 frontend tests, strict TypeScript,
  the production build, and the dogfood harness. Isolated desktop and Android
  fixture proof exercised submission, queued state, and rejection presentation
  with no target-worker exposure. The deployed public Keeper surface rendered at
  1,440 by 900 and 412 by 915 with zero horizontal overflow and no browser
  warnings or errors. The package created a verified 1,183,744-byte pre-update
  backup and migrated the live database to schema 58 with `quick_check=ok`. API
  PID advanced to `3891701`; terminal-host PID `2966127` and the active Queen
  session remained unchanged. No Jira or live Apiary mutation occurred, and the
  proof browser closed after the run.
- Release `0.1.0-dev-b05442a088ec` gives Keeper a bounded, privacy-preserving
  audit of Steward task routing. Accepted shared tasks identify the routing
  Steward, while the delegation surface shows recent accepted and declined
  actions without exposing a member Hive's workers, repositories, terminals, or
  credentials. The full gate passed 382 Rust tests, warnings-denied Clippy,
  formatting, all 235 frontend tests, strict TypeScript, the production build,
  and the dogfood harness. Isolated desktop and Android-size fixtures proved the
  populated audit and rolling-update fallback. The deployed public Keeper view
  rendered at 1,440 by 900 and 412 by 915 with zero horizontal overflow and no
  browser warnings or errors. The package created a verified 1,208,320-byte
  pre-update backup; schema 58 remained healthy with `quick_check=ok`. API PID
  advanced to `3902142`; terminal-host PID `2966127`, worker-engine build
  `f8abd293`, and the active Queen session remained unchanged. No Jira or live
  Apiary mutation occurred, and the proof browser closed after the run.
- Release `0.1.0-dev-c592274d42b1` adds a privacy-bounded Steward Observe
  pulse. A Member Steward sees only per-managed-Hive counts for Ready, Active,
  Blocked, Review, and active Jira claims plus the latest shared-work activity
  time. Private workers, repositories, terminals, transcripts, local tasks,
  provider sessions, Jira issue content, and credentials remain absent. The
  full gate passed 382 Rust tests, warnings-denied Clippy, formatting, all 235
  frontend tests, strict TypeScript, the production build, and the dogfood
  harness. The actual Member component rendered cleanly at 1,440 by 900 and
  Android-size 412 by 915 with zero horizontal overflow; rolling-update tests
  also proved both older Keeper and newer Member payloads. The deployed public
  surface reported exact release `0.1.0-dev-c592274d42b1-20260816093833-3915254`
  at both sizes with zero horizontal overflow and no browser warnings. Schema
  58 remained healthy with `quick_check=ok` and a 1,208,320-byte database. API
  PID advanced to `3916638`; terminal-host PID `2966127`, worker-engine build
  `f8abd293`, and Queen session `01a004fc-68b6-7720-9a6e-0eebf2434db1`
  remained unchanged. No Jira or live Apiary mutation occurred, and the proof
  browser closed after the run.
- Release `0.1.0-dev-b0d74982e82b` adds operator-reviewed Steward Assist.
  A scoped Steward can queue a short help offer for one managed Hive; Keeper
  authenticates the Member, rechecks the exact current Assist grant and target,
  and records one retry-stable result. The target operator accepts or declines
  from her own Hive. No path injects a terminal, starts a worker, changes an
  engagement lease, or implies takeover. Incoming, sent, queued, accepted, and
  declined states remain durable across outbound Member polling. The complete
  gate passed 386 Rust tests, warnings-denied Clippy, formatting, all 236
  frontend tests, strict TypeScript, the production build, and the dogfood
  harness. The actual populated Member component rendered at 1,440 by 900 and
  Android-size 412 by 915 with zero horizontal overflow; all mobile Assist
  controls met a 44-pixel touch target, and Accept and Offer completed without
  UI errors. The deployed public Keeper and Apiary surfaces reported exact
  release `0.1.0-dev-b0d74982e82b-20260816102112-3939579`, rendered with zero
  horizontal overflow, and emitted no browser warnings or errors. The live
  database migrated to schema 59 with `quick_check=ok`, all four Assist journal
  tables present, and a 1,208,320-byte database. API PID advanced to `3940991`;
  terminal-host PID `2966127`, worker-engine build `f8abd293`, and Queen session
  `01a004fc-68b6-7720-9a6e-0eebf2434db1` remained unchanged. No Jira or live
  Apiary mutation occurred, and every proof browser closed after the run.
- Release `0.1.0-dev-cda6410b3825` makes pending Steward help visible in the
  Member operator's normal control room. Pending offers contribute to **Needs
  you**, add a compact Apiary badge, and open the exact Apiary review surface;
  the copy explicitly says that no worker or terminal was interrupted. Member
  polling emits one content-free live-feed invalidation only when the bounded
  Assist projection changes, so unchanged polls do not create event churn.
  Personal Hives do not request the federated projection. The complete gate
  passed 386 Rust tests, warnings-denied Clippy, formatting, all 238 frontend
  tests, strict TypeScript, the production build, and the dogfood harness. The
  actual attention component rendered at 1,440 by 900 and Android-size 412 by
  915 with zero horizontal overflow; the mobile action measured 44 pixels and
  navigated to Member Hive review. The deployed public Keeper and Apiary
  surfaces reported exact release
  `0.1.0-dev-cda6410b3825-20260816104056-3950888`, rendered with zero
  horizontal overflow, and emitted no browser warnings or errors. Schema 59
  remained healthy with `quick_check=ok` and a 1,257,472-byte database. API PID
  advanced to `3952145`; terminal-host PID `2966127` and worker-engine build
  `f8abd293` remained unchanged. No Jira or live Apiary mutation occurred, and
  every proof browser closed after the run.
- Main commit `cd46fc27` and host-compatible release
  `0.1.0-26b22a4aaca9` add deterministic attention for Active work whose
  assigned worker process has exited. The rule waits through the recovery
  window, rechecks task revision and replacement-session state, and creates at
  most one current record for the newest process incarnation. It does not
  restart a worker, transition a task, inject a terminal, or mutate Jira. The
  complete gate passed 413 Rust tests, warnings-denied Clippy, formatting, all
  244 frontend tests, strict TypeScript, and the production build. The live
  Queen settings rendered at desktop and Android-size 412 by 915 with the new
  **Work surfaced** metric and zero horizontal overflow. Automatic Queen review
  remained off. The live database migrated to schema 64 with
  `quick_check=ok`; no production attention record was fabricated. API PID
  advanced to `79485`; terminal-host PID `2966127`, worker-engine build
  `f8abd293`, and Queen provider PID `2966160` remained unchanged. No Jira or
  live-task mutation occurred, and the temporary viewport override was reset.
- Main commit `8540fcd2` and host-compatible release
  `0.1.0-f43ec1d66951` serialize deterministic sleeping-worker starts. One
  coordination pass claims one wake; every additional Queen-originated Ready
  assignment remains durable until a later pass samples the changed process
  tree. Manual starts remain immediate. The complete gate passed 414 Rust
  tests, warnings-denied Clippy, formatting, all 244 frontend tests, strict
  TypeScript, and the production build. The live coordinator contract reported
  `automatic_start_batch_limit=1`, normal admission, no queued or uncertain
  action, and automatic Queen review remained off. The actual Settings surface
  rendered at desktop and Android-size 412 by 915 with the memory-recheck
  explanation and zero horizontal overflow. Schema 64 remained healthy with
  `quick_check=ok`. API PID advanced to `92444`; terminal-host PID `2966127`,
  worker-engine build `f8abd293`, and Queen provider PID `2966160` remained
  unchanged. No Jira, task, worker, or Apiary mutation occurred, and the
  temporary viewport override was reset.
- Main commit `5356d09e` and host-compatible release
  `0.1.0-20eb322d5139` restore mobile terminal scrolling on Android Chromium's
  primary pointer-event path. Swarm captures one primary touch pointer, owns
  the drag until release, and prevents xterm's document gesture recognizer
  from reinterpreting the same movement. Older WebKit retains the bounded
  TouchEvent fallback, while mouse selection and desktop wheel behavior are
  unchanged. The regression exercises the real child-to-capture propagation
  path and proves one drag produces one scroll operation without reaching the
  competing downstream listener. The complete gate passed 414 Rust tests,
  warnings-denied Clippy, formatting, all 245 frontend tests, strict
  TypeScript, and the production build. The deployed terminal rendered at
  Android-size 412 by 915 and desktop 1,142 by 888 with zero horizontal
  overflow; its live canonical buffer exposed 1,000 scrollback rows. API PID
  advanced to `101779`; terminal-host PID `2966127`, worker-engine build
  `f8abd293`, and Queen provider PID `2966160` remained unchanged. Database
  `quick_check` remained `ok`; no Jira, task, worker, or Apiary mutation
  occurred, and the temporary viewport override was reset.
- Main commit `d814f868` and host-compatible release
  `0.1.0-f3260fa27b7d` prevent Queen from moving Ready or Blocked work to
  Active while its assigned worker is unloaded. The application transition is
  guarded atomically against the exact assigned live session, and the direct
  Queen MCP path proves a sleeping assignment remains Ready until that worker
  has been loaded. The complete gate passed 416 Rust tests, warnings-denied
  Clippy, formatting, all 245 frontend tests, strict TypeScript, and the
  production build. The live Queen settings rendered at desktop 1,142 by 888
  and Android-size 412 by 915 with the new wake-before-Active explanation and
  zero horizontal overflow. Automatic Queen review remained off. API PID
  advanced to `113653`; terminal-host PID `2966127`, worker-engine build
  `f8abd293`, and Queen provider PID `2966160` remained unchanged. Database
  `quick_check` remained `ok`; no Jira, task, worker, or Apiary mutation
  occurred, and the temporary viewport override was reset.
- The isolated production-shaped Queen journey now proves the previously
  separate handoff and conductor contracts as one chain. A disposable worker
  owns a Ready task, advances it through Active to Review with verification
  evidence, and the real terminal host delivers both the worker outcome and
  exactly one bounded actionable-work review to the disposable Queen. The
  exact run then closes as Completed. The prompt explicitly denies Jira,
  Apiary, email, deployment, and other external effects. The API quality gate
  passed warnings-denied Clippy and all 149 API binary and library tests. Live
  automatic review remains off until the operator chooses a bounded real task.
- Main commits `f7ffc4ee` and `76009441`, deployed as host-compatible release
  `0.1.0-f607df2c6d41`, bind same-port development reload to the exact product
  source revision instead of trusting whichever checkout happens to be
  configured. The package records source revision `76009441739d`; the API
  rejects missing, older, or unrelated source and ignores stale legacy reload
  markers. The System surface separately identifies the compatibility package
  and the active product revision, so an operator no longer mistakes a package
  commit for the source being dogfooded. The complete gate passed 426 Rust
  tests, warnings-denied Clippy, formatting, all 259 frontend tests, strict
  TypeScript, the production build, and the full Linux package lifecycle. Live
  System and Queen settings rendered at 1,440 by 900 and Android-size 412 by
  915 with zero horizontal overflow and no alerts. The active revision showed
  `7600944`, Queen remained **Automatic off / Manual review only**, and no
  external-effect path was enabled. API release activation preserved terminal
  holder PID `2966127`, Claude provider PID `97543`, Queen provider PID
  `2966160`, and worker-engine build `f8abd293`. Database `quick_check` remained
  `ok`, the viewport override was reset, and the temporary proof tab was
  closed.
- Main commit `a0468faf`, deployed as host-compatible release
  `0.1.0-a566cef69af4`, makes the device with the current operator engagement
  lease the sole authority for shared PTY geometry. Passive desktop and mobile
  viewers remember their own fitted size without resizing the provider; the
  first input from another device atomically transfers the engagement and
  applies that device's latest bounded dimensions before its bytes. A real-PTY
  regression proves passive desktop and phone attachments cannot fight, while
  explicit input transfers authority in both directions. The complete gate
  passed 429 Rust tests, warnings-denied Clippy, formatting, all 259 frontend
  tests, strict TypeScript, and the production build. The deployed Queen
  terminal remained connected and populated through Android-size 412 by 915
  and back to the default 1,465 by 1,339 desktop viewport with zero horizontal
  overflow and 66 rendered rows at both sizes; the proof tab emitted no browser
  warnings or errors. API PID advanced to `445611`; terminal-host PID
  `2966127`, worker-engine build `f8abd293`, Claude provider PID `97543`, and
  Queen provider PID `2966160` remained unchanged. Database `quick_check`
  remained `ok`; no input, Jira, task, worker, or Apiary mutation occurred, the
  viewport override was reset, and the temporary proof tab was closed.
- Main commits `382909ec`, `9615af0a`, `2b6379a2`, and `568038fc`, deployed as
  host-compatible release `0.1.0-ebaece8f7e20`, add a read-only task-detail
  dialog without making the dense board rows taller. Double-clicking a task or
  choosing **View details** shows full Jira metadata, description text, bounded
  attachment metadata, and authenticated image previews; **Edit** remains a
  separate mutation path. Attachment bytes are scoped back to the exact linked
  issue, capped at 15 MiB, restricted to raster media, and checked against the
  file signature before Swarm serves them. Live proof exposed two Atlassian
  transport requirements that the mock alone could not: the content endpoint
  must use `redirect=false`, and that form rejects a media-specific `Accept`
  header with HTTP 406. The corrected request now returns the real 39,287-byte
  PNG for WWD-4976. The feature gate passed 430 Rust tests, all 261 frontend
  tests, strict TypeScript, the production build, formatting, and
  warnings-denied Clippy; the follow-up transport fix passed all 151 API tests
  and warnings-denied Clippy. A reusable read-only Chromium smoke then decoded
  the real 642 by 386 image at desktop 1,440 by 900 and Android-size 412 by 915.
  The dialog remained inside each viewport and both surfaces had zero
  horizontal page overflow or browser errors. API PID advanced to `509824`;
  terminal-holder PID `2966127` and worker-engine build `f8abd293` remained
  unchanged. Database `quick_check` remained `ok`; no Jira, task, worker, or
  Apiary mutation occurred, and the private proof tab was closed.
