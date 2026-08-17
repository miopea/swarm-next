# Legacy regression-chain candidates

Generated from commit subjects and dates. Candidates share their primary subject capability and occur within four days. These are review candidates, not proof of causality; each accepted chain must be checked against its diff and final stable replacement.

## d7ef6ff8 — feat: fullscreen terminal button for mobile scrollback

- Date: 2026-02-17
- Subject capability overlap: terminal, mobile_pwa
- Corrective follow-ups within four days: 18
  - fe48aeb0 (2026-02-17): fix: reuse existing terminal in fullscreen, fix scroll direction and speed
  - 233f17c3 (2026-02-17): fix: queen notification banners overlap terminal content
  - 70fd5749 (2026-02-17): fix: Ctrl+C copies selected text to clipboard in terminal
  - 9f355401 (2026-02-17): fix: clipboard copy from tmux copy-mode via server-side buffer read
  - 857b3f51 (2026-02-18): perf(drones): batch tmux calls and skip redundant poll work
  - ad065ed2 (2026-02-18): perf(tmux): reuse batch_pane_info snapshot in update_window_names
  - a9fc586f (2026-02-19): fix: strip CLAUDECODE env from tmux sessions, mock slow test calls
  - 6c8e8afc (2026-02-19): fix(ui): fullscreen terminal fills entire viewport on mobile
  - 0f41d62d (2026-02-20): fix(pty): null checks, base64 safety, race condition, stale docs
  - b203c945 (2026-02-20): fix(pty): runtime fixes from production testing
  - 7540f7cb (2026-02-20): fix(pty): holder death notifications, spurious revive fix, tmux cleanup
  - 98f50970 (2026-02-20): fix(terminal): prevent WS disconnect when switching workers
  - ... 6 more candidates remain in the complete ledger

## 9b87c6a5 — feat: add xterm ClipboardAddon for tmux copy-mode clipboard support

- Date: 2026-02-17
- Subject capability overlap: terminal
- Corrective follow-ups within four days: 16
  - 9f355401 (2026-02-17): fix: clipboard copy from tmux copy-mode via server-side buffer read
  - 857b3f51 (2026-02-18): perf(drones): batch tmux calls and skip redundant poll work
  - ad065ed2 (2026-02-18): perf(tmux): reuse batch_pane_info snapshot in update_window_names
  - a9fc586f (2026-02-19): fix: strip CLAUDECODE env from tmux sessions, mock slow test calls
  - 6c8e8afc (2026-02-19): fix(ui): fullscreen terminal fills entire viewport on mobile
  - 0f41d62d (2026-02-20): fix(pty): null checks, base64 safety, race condition, stale docs
  - b203c945 (2026-02-20): fix(pty): runtime fixes from production testing
  - 7540f7cb (2026-02-20): fix(pty): holder death notifications, spurious revive fix, tmux cleanup
  - 98f50970 (2026-02-20): fix(terminal): prevent WS disconnect when switching workers
  - f23d6006 (2026-02-20): fix(terminal): strip partial ANSI sequences from ring buffer snapshot
  - 21b1c087 (2026-02-20): fix(terminal): cache xterm.js instances to preserve scrollback across worker switches
  - 2ad4b9ab (2026-02-21): fix(dashboard): reset xterm.js before WS reconnect to preserve terminal history
  - ... 4 more candidates remain in the complete ledger

## c7e2efb7 — feat(pty): replace tmux with direct PTY management layer

- Date: 2026-02-20
- Subject capability overlap: terminal
- Corrective follow-ups within four days: 15
  - 0f41d62d (2026-02-20): fix(pty): null checks, base64 safety, race condition, stale docs
  - b203c945 (2026-02-20): fix(pty): runtime fixes from production testing
  - 7540f7cb (2026-02-20): fix(pty): holder death notifications, spurious revive fix, tmux cleanup
  - 98f50970 (2026-02-20): fix(terminal): prevent WS disconnect when switching workers
  - f23d6006 (2026-02-20): fix(terminal): strip partial ANSI sequences from ring buffer snapshot
  - 21b1c087 (2026-02-20): fix(terminal): cache xterm.js instances to preserve scrollback across worker switches
  - 2ad4b9ab (2026-02-21): fix(dashboard): reset xterm.js before WS reconnect to preserve terminal history
  - f1740a94 (2026-02-21): fix(pty): prepend alternate screen sequence in snapshot when rolled off buffer
  - 8b29c1c8 (2026-02-21): fix(dashboard): sync scrollbar position after terminal re-show and WS reconnect
  - 511b6c53 (2026-02-21): fix(dashboard): use ResizeObserver for stable terminal layout on worker switch
  - 2be43d03 (2026-02-21): fix(dashboard): refresh button now resizes PTY to current viewport
  - 895e0e97 (2026-02-22): fix(pty): resolve nvm/cargo/deno paths in holder child env
  - ... 3 more candidates remain in the complete ledger

## 8be14499 — Add drag-and-drop file upload to inline and modal terminals

- Date: 2026-02-10
- Subject capability overlap: terminal, resources, ui_ux
- Corrective follow-ups within four days: 12
  - e936fe5d (2026-02-10): Fix tmux not resizing when browser window resizes in WUI terminal
  - 91c3e75c (2026-02-10): Fix tmux resize: send SIGWINCH directly instead of refresh-client -C
  - 704a0afa (2026-02-10): Fix paste and image drop in WUI terminal
  - 06a593d7 (2026-02-10): Fix mouse click-to-select-pane in WUI terminal via onBinary handler
  - 7b4b6f6f (2026-02-10): Revert onBinary and mousedown pre-focus handlers causing terminal regressions
  - afecdd99 (2026-02-10): Revert all terminal handler changes — restore to pre-regression state
  - d5f528f4 (2026-02-10): Fix stale WS close race + add terminal debug logging
  - 1ff60d06 (2026-02-10): Restore SIGWINCH on terminal resize — fixes mouse click coordinates
  - 7c41fc49 (2026-02-12): Fix CI: include PaneGoneError/TmuxError in narrowed exceptions, fix mock targets
  - 43e778a2 (2026-02-13): fix: enable Ctrl-V paste in tmux swarm sessions (#36)
  - cb2be02b (2026-02-13): fix: restore Ctrl-V paste interception in web UI terminals (#36)
  - b75f133d (2026-02-13): fix: launch workers into existing tmux session instead of replacing it

## 29460389 — feat: add task state backup with rotation and rate limiting tests

- Date: 2026-03-30
- Subject capability overlap: tasks, testing_quality
- Corrective follow-ups within four days: 10
  - 3fcef2dd (2026-03-30): fix: ownership check_overlap signature, cross-task completion + auto-assign
  - 4bd27ede (2026-03-31): fix: remove task-search class from worker search input
  - d44ee3e7 (2026-04-01): fix: disable speculation — injecting unrelated tasks into wrong workers
  - e323a517 (2026-04-01): fix: MCP create_task now dispatches to worker via assign_task
  - 5ff06199 (2026-04-01): fix: task dispatch prefers MCP swarm_complete_task over file-based completion
  - 5c69835e (2026-04-02): fix: complete DB migration — auth secrets, CLI tasks, mtime watcher
  - 3bf1a59f (2026-04-02): fix: cancel DB maintenance task on shutdown + clear secrets on disconnect
  - 22dea759 (2026-04-02): fix: refresh task/buzz panels on WS reconnect and tab focus
  - a2678331 (2026-04-02): fix: SqliteTaskStore.save() now deletes removed tasks from DB
  - 41eb85d4 (2026-04-03): fix: show all task fields in create and edit modal consistently

## e9d7a810 — feat: add task dependency title tooltips and resource history API

- Date: 2026-03-30
- Subject capability overlap: tasks, resources
- Corrective follow-ups within four days: 10
  - 3fcef2dd (2026-03-30): fix: ownership check_overlap signature, cross-task completion + auto-assign
  - 4bd27ede (2026-03-31): fix: remove task-search class from worker search input
  - d44ee3e7 (2026-04-01): fix: disable speculation — injecting unrelated tasks into wrong workers
  - e323a517 (2026-04-01): fix: MCP create_task now dispatches to worker via assign_task
  - 5ff06199 (2026-04-01): fix: task dispatch prefers MCP swarm_complete_task over file-based completion
  - 5c69835e (2026-04-02): fix: complete DB migration — auth secrets, CLI tasks, mtime watcher
  - 3bf1a59f (2026-04-02): fix: cancel DB maintenance task on shutdown + clear secrets on disconnect
  - 22dea759 (2026-04-02): fix: refresh task/buzz panels on WS reconnect and tab focus
  - a2678331 (2026-04-02): fix: SqliteTaskStore.save() now deletes removed tasks from DB
  - 41eb85d4 (2026-04-03): fix: show all task fields in create and edit modal consistently

## b90eb4fa — feat(config): add setup guide for Jira token auth mode

- Date: 2026-03-02
- Subject capability overlap: jira, security_auth, settings
- Corrective follow-ups within four days: 9
  - bded0267 (2026-03-02): fix(jira): remove token auth, fix doubled URL, persist OAuth credentials
  - 26f64666 (2026-03-02): fix(jira): persist client_secret to YAML, remove default status filter
  - 7d10e505 (2026-03-02): fix(jira): add 410 recovery, log accessible resources, add Accept header
  - 82afedae (2026-03-02): fix(jira): add warning log to preview endpoint for API errors
  - b8eb70e8 (2026-03-03): fix(jira): migrate to /rest/api/3/search/jql endpoint
  - 39cee6f0 (2026-03-03): fix(jira): valid fallback JQL, hot-reload log level on config save
  - af20a34e (2026-03-03): fix: add success log for Jira completion comments
  - f59e96cb (2026-03-03): fix(jira): retry account_id discovery on token refresh when missing
  - fa46c164 (2026-03-04): fix(config): preserve Jira credentials on save, widen inputs, add existing-app guidance

## a9febebe — feat(jira): complete Jira integration — OAuth, two-way sync, config UI, dashboard buttons

- Date: 2026-03-02
- Subject capability overlap: jira, security_auth, settings, ui_ux
- Corrective follow-ups within four days: 9
  - bded0267 (2026-03-02): fix(jira): remove token auth, fix doubled URL, persist OAuth credentials
  - 26f64666 (2026-03-02): fix(jira): persist client_secret to YAML, remove default status filter
  - 7d10e505 (2026-03-02): fix(jira): add 410 recovery, log accessible resources, add Accept header
  - 82afedae (2026-03-02): fix(jira): add warning log to preview endpoint for API errors
  - b8eb70e8 (2026-03-03): fix(jira): migrate to /rest/api/3/search/jql endpoint
  - 39cee6f0 (2026-03-03): fix(jira): valid fallback JQL, hot-reload log level on config save
  - af20a34e (2026-03-03): fix: add success log for Jira completion comments
  - f59e96cb (2026-03-03): fix(jira): retry account_id discovery on token refresh when missing
  - fa46c164 (2026-03-04): fix(config): preserve Jira credentials on save, widen inputs, add existing-app guidance

## 4a380297 — feat(jira): OAuth-first config, case-insensitive label filter, better preview errors

- Date: 2026-03-02
- Subject capability overlap: jira, security_auth, settings, testing_quality
- Corrective follow-ups within four days: 9
  - bded0267 (2026-03-02): fix(jira): remove token auth, fix doubled URL, persist OAuth credentials
  - 26f64666 (2026-03-02): fix(jira): persist client_secret to YAML, remove default status filter
  - 7d10e505 (2026-03-02): fix(jira): add 410 recovery, log accessible resources, add Accept header
  - 82afedae (2026-03-02): fix(jira): add warning log to preview endpoint for API errors
  - b8eb70e8 (2026-03-03): fix(jira): migrate to /rest/api/3/search/jql endpoint
  - 39cee6f0 (2026-03-03): fix(jira): valid fallback JQL, hot-reload log level on config save
  - af20a34e (2026-03-03): fix: add success log for Jira completion comments
  - f59e96cb (2026-03-03): fix(jira): retry account_id discovery on token refresh when missing
  - fa46c164 (2026-03-04): fix(config): preserve Jira credentials on save, widen inputs, add existing-app guidance

## 7589330b — feat: add bulk task operations API and multi-select UI

- Date: 2026-03-30
- Subject capability overlap: tasks, ui_ux
- Corrective follow-ups within four days: 9
  - 3fcef2dd (2026-03-30): fix: ownership check_overlap signature, cross-task completion + auto-assign
  - 4bd27ede (2026-03-31): fix: remove task-search class from worker search input
  - d44ee3e7 (2026-04-01): fix: disable speculation — injecting unrelated tasks into wrong workers
  - e323a517 (2026-04-01): fix: MCP create_task now dispatches to worker via assign_task
  - 5ff06199 (2026-04-01): fix: task dispatch prefers MCP swarm_complete_task over file-based completion
  - 5c69835e (2026-04-02): fix: complete DB migration — auth secrets, CLI tasks, mtime watcher
  - 3bf1a59f (2026-04-02): fix: cancel DB maintenance task on shutdown + clear secrets on disconnect
  - 22dea759 (2026-04-02): fix: refresh task/buzz panels on WS reconnect and tab focus
  - a2678331 (2026-04-02): fix: SqliteTaskStore.save() now deletes removed tasks from DB

## 1ae8dc0f — feat(pty): shell_wrap — workers survive CLI exit with login shell fallback

- Date: 2026-02-22
- Subject capability overlap: terminal, workers, recovery
- Corrective follow-ups within four days: 8
  - 8a27eb22 (2026-02-23): fix(task,terminal): prep_for_task sees shell_wrap workers as STUNG; duplicate keystrokes on WS reconnect
  - 120c9f12 (2026-02-23): fix(pty): raise StreamReader limit to 2MB to prevent LimitOverrunError on large snapshots
  - ee46bc2e (2026-02-24): fix(web): send SIGWINCH on terminal refresh to force TUI redraw
  - d547b6e2 (2026-02-25): fix(pty): add terminal-active guard to prevent input injection while user types
  - bc8dc7c2 (2026-02-25): fix(pty): add command ID to pool↔holder protocol and fix escalation tracking
  - d3b37d8d (2026-02-25): fix(security): deep scan hardening — XSS, WS origin, PTY cleanup, confidence gates
  - 4f04990d (2026-02-25): fix(drones): block Queen continue on empty prompts
  - e43025e9 (2026-02-26): Fix CI lint errors from terminal stability changes

## 7887087a — feat(dashboard): worker tiles show which task, and whether it is started (#1496)

- Date: 2026-08-11
- Subject capability overlap: tasks, workers, ui_ux
- Corrective follow-ups within four days: 8
  - d29c3c63 (2026-08-12): fix(goals): clear an armed native /goal when its task stops being active (#1536)
  - 52eea9f5 (2026-08-12): fix(queen): stop claiming a dispatch happened, and sweep the stranded row (#1527)
  - f649ecb8 (2026-08-13): fix(mcp): refuse a task routed to a worker that does not exist (#1543)
  - 567e8de7 (2026-08-13): fix(mcp): swarm_create_task now applies the priority it was given (#1543)
  - febc8624 (2026-08-14): fix(drones): nudge suppression keys on state_duration, not MCP dispatches (#1615)
  - 72c66362 (2026-08-14): fix(queen): interrupt reports a dispatch, not an outcome (#1608)
  - 16dde76b (2026-08-14): fix(tasks): an ASSIGNED-and-BACKLOG task had no legal close route (#1636)
  - f01a065a (2026-08-15): fix(drones): stop nudging workers whose task is ACTIVE and they're working (#1664)

## f4643c6e — feat(drones): detect operator terminal approvals + "Approve Always" rules

- Date: 2026-02-27
- Subject capability overlap: terminal, drones
- Corrective follow-ups within four days: 6
  - 23151d91 (2026-02-27): fix(drones): extract approval pattern from raw PTY content, not summary
  - de1763d1 (2026-02-28): fix(pty,drones): close 7 concurrency race conditions
  - b0117d1d (2026-03-01): fix(detection): fix plan detection, dedup queen banners, hide empty Custom Rule
  - 95c82311 (2026-03-01): fix(terminal): restore full scrollback on reload and fix mobile viewport race
  - 65bb2147 (2026-03-02): fix(jira): handle empty project in JQL builder
  - d913421c (2026-03-03): fix(jira): merge status_map with defaults so empty {} doesn't break transitions

## ee54b7fc — feat(messaging)!: broadcast is Queen-only

- Date: 2026-08-12
- Subject capability overlap: queen, messaging
- Corrective follow-ups within four days: 6
  - 58339e99 (2026-08-14): fix(queen): report the hold instead of claiming delivery, and record that interrupt does not close a picker (#1608)
  - 85491fc4 (2026-08-14): fix(queen): read back before claiming a prompt was answered (#1608)
  - af324d2c (2026-08-14): fix(queen): answer a picker with arrows and Enter, never by typing the digit (#1608)
  - 78c1ef4d (2026-08-14): fix(queen): dismiss reads back, and stops claiming Escape works (#1623)
  - 314f05bf (2026-08-15): fix(queen): dismiss is OBSERVED to work, and declines rather than commits (#1623)
  - b338f1c8 (2026-08-15): fix(queen): a refused prompt is now recoverable on both sides (#1648)

## be122e22 — feat(queen): answer and dismiss an open selection prompt (#1608)

- Date: 2026-08-14
- Subject capability overlap: queen
- Corrective follow-ups within four days: 6
  - 58339e99 (2026-08-14): fix(queen): report the hold instead of claiming delivery, and record that interrupt does not close a picker (#1608)
  - 85491fc4 (2026-08-14): fix(queen): read back before claiming a prompt was answered (#1608)
  - af324d2c (2026-08-14): fix(queen): answer a picker with arrows and Enter, never by typing the digit (#1608)
  - 78c1ef4d (2026-08-14): fix(queen): dismiss reads back, and stops claiming Escape works (#1623)
  - 314f05bf (2026-08-15): fix(queen): dismiss is OBSERVED to work, and declines rather than commits (#1623)
  - b338f1c8 (2026-08-15): fix(queen): a refused prompt is now recoverable on both sides (#1648)

## 7650262b — Add mouse escape sequence logging to modal terminal onData

- Date: 2026-02-10
- Subject capability overlap: terminal, ui_ux
- Corrective follow-ups within four days: 5
  - 1ff60d06 (2026-02-10): Restore SIGWINCH on terminal resize — fixes mouse click coordinates
  - 7c41fc49 (2026-02-12): Fix CI: include PaneGoneError/TmuxError in narrowed exceptions, fix mock targets
  - 43e778a2 (2026-02-13): fix: enable Ctrl-V paste in tmux swarm sessions (#36)
  - cb2be02b (2026-02-13): fix: restore Ctrl-V paste interception in web UI terminals (#36)
  - b75f133d (2026-02-13): fix: launch workers into existing tmux session instead of replacing it

## 8d42a1d7 — Add task lifecycle automation, skill-based workflows, and Queen directives

- Date: 2026-02-10
- Subject capability overlap: tasks, queen, workers
- Corrective follow-ups within four days: 5
  - c72a0624 (2026-02-11): Fix confirm dialog callback never firing, allow removing completed tasks
  - d54d7823 (2026-02-11): Fix selected worker task text invisible, show task in detail title
  - 788539ee (2026-02-11): Prevent coordination cycle from assigning tasks to busy workers
  - c518f3c7 (2026-02-14): fix: prevent drones auto-disabling on stale task completions + revert CSP
  - d6b37949 (2026-02-14): fix: resolve test-mode auto-assign failures and improve drone decision pipeline

## 46611ec7 — Add configurable workflow skills and pre-task worker prep

- Date: 2026-02-11
- Subject capability overlap: tasks, workers, settings
- Corrective follow-ups within four days: 5
  - c72a0624 (2026-02-11): Fix confirm dialog callback never firing, allow removing completed tasks
  - d54d7823 (2026-02-11): Fix selected worker task text invisible, show task in detail title
  - 788539ee (2026-02-11): Prevent coordination cycle from assigning tasks to busy workers
  - c518f3c7 (2026-02-14): fix: prevent drones auto-disabling on stale task completions + revert CSP
  - d6b37949 (2026-02-14): fix: resolve test-mode auto-assign failures and improve drone decision pipeline

## 182428c6 — Add task resolution field and workflows config editor

- Date: 2026-02-11
- Subject capability overlap: tasks, settings
- Corrective follow-ups within four days: 5
  - c72a0624 (2026-02-11): Fix confirm dialog callback never firing, allow removing completed tasks
  - d54d7823 (2026-02-11): Fix selected worker task text invisible, show task in detail title
  - 788539ee (2026-02-11): Prevent coordination cycle from assigning tasks to busy workers
  - c518f3c7 (2026-02-14): fix: prevent drones auto-disabling on stale task completions + revert CSP
  - d6b37949 (2026-02-14): fix: resolve test-mode auto-assign failures and improve drone decision pipeline

## 44aa23e8 — Add task numbers, unassign task on failed send after /clear

- Date: 2026-02-11
- Subject capability overlap: tasks
- Corrective follow-ups within four days: 5
  - c72a0624 (2026-02-11): Fix confirm dialog callback never firing, allow removing completed tasks
  - d54d7823 (2026-02-11): Fix selected worker task text invisible, show task in detail title
  - 788539ee (2026-02-11): Prevent coordination cycle from assigning tasks to busy workers
  - c518f3c7 (2026-02-14): fix: prevent drones auto-disabling on stale task completions + revert CSP
  - d6b37949 (2026-02-14): fix: resolve test-mode auto-assign failures and improve drone decision pipeline

## 139fd03a — Add task reopen + prevent false Queen completions

- Date: 2026-02-11
- Subject capability overlap: tasks, queen
- Corrective follow-ups within four days: 5
  - c72a0624 (2026-02-11): Fix confirm dialog callback never firing, allow removing completed tasks
  - d54d7823 (2026-02-11): Fix selected worker task text invisible, show task in detail title
  - 788539ee (2026-02-11): Prevent coordination cycle from assigning tasks to busy workers
  - c518f3c7 (2026-02-14): fix: prevent drones auto-disabling on stale task completions + revert CSP
  - d6b37949 (2026-02-14): fix: resolve test-mode auto-assign failures and improve drone decision pipeline

## 5b75148c — feat(providers): wire provider config through worker lifecycle

- Date: 2026-02-21
- Subject capability overlap: workers, settings, providers
- Corrective follow-ups within four days: 5
  - 22aa2bbb (2026-02-22): fix(ui): PWA badge shows decisions only, redesign workers config
  - f1d71d7e (2026-02-22): fix(ui): add CSRF header to duplicate-as spawn fetch
  - 3432b207 (2026-02-22): fix(spawn): auto-start spawned workers with correct provider CLI
  - bc5c80ec (2026-02-22): fix(config): inherit description when saving spawned worker
  - 2a6737ec (2026-02-25): fix(web): remove cost badges from dashboard worker cards

## 2519b462 — feat(providers): add TerminalEvent types and parse_events() base method

- Date: 2026-02-28
- Subject capability overlap: terminal, providers
- Corrective follow-ups within four days: 5
  - b0117d1d (2026-03-01): fix(detection): fix plan detection, dedup queen banners, hide empty Custom Rule
  - 95c82311 (2026-03-01): fix(terminal): restore full scrollback on reload and fix mobile viewport race
  - 65bb2147 (2026-03-02): fix(jira): handle empty project in JQL builder
  - d913421c (2026-03-03): fix(jira): merge status_map with defaults so empty {} doesn't break transitions
  - a89e864b (2026-03-03): fix: production readiness audit — harden PTY, drones, server, and frontend

## b6f81f8e — feat(pty): add custom TerminalEmulator replacing pyte

- Date: 2026-02-28
- Subject capability overlap: terminal
- Corrective follow-ups within four days: 5
  - b0117d1d (2026-03-01): fix(detection): fix plan detection, dedup queen banners, hide empty Custom Rule
  - 95c82311 (2026-03-01): fix(terminal): restore full scrollback on reload and fix mobile viewport race
  - 65bb2147 (2026-03-02): fix(jira): handle empty project in JQL builder
  - d913421c (2026-03-03): fix(jira): merge status_map with defaults so empty {} doesn't break transitions
  - a89e864b (2026-03-03): fix: production readiness audit — harden PTY, drones, server, and frontend

## 888ad305 — feat(terminal): add Search and WebLinks xterm addons

- Date: 2026-02-28
- Subject capability overlap: terminal
- Corrective follow-ups within four days: 5
  - b0117d1d (2026-03-01): fix(detection): fix plan detection, dedup queen banners, hide empty Custom Rule
  - 95c82311 (2026-03-01): fix(terminal): restore full scrollback on reload and fix mobile viewport race
  - 65bb2147 (2026-03-02): fix(jira): handle empty project in JQL builder
  - d913421c (2026-03-03): fix(jira): merge status_map with defaults so empty {} doesn't break transitions
  - a89e864b (2026-03-03): fix: production readiness audit — harden PTY, drones, server, and frontend

## 54debb6e — feat(terminal): add WebGL + Canvas GPU-accelerated rendering

- Date: 2026-02-28
- Subject capability overlap: terminal
- Corrective follow-ups within four days: 5
  - b0117d1d (2026-03-01): fix(detection): fix plan detection, dedup queen banners, hide empty Custom Rule
  - 95c82311 (2026-03-01): fix(terminal): restore full scrollback on reload and fix mobile viewport race
  - 65bb2147 (2026-03-02): fix(jira): handle empty project in JQL builder
  - d913421c (2026-03-03): fix(jira): merge status_map with defaults so empty {} doesn't break transitions
  - a89e864b (2026-03-03): fix: production readiness audit — harden PTY, drones, server, and frontend

## c49f1fee — feat(terminal): add onBell and onTitleChange event handlers

- Date: 2026-02-28
- Subject capability overlap: terminal
- Corrective follow-ups within four days: 5
  - b0117d1d (2026-03-01): fix(detection): fix plan detection, dedup queen banners, hide empty Custom Rule
  - 95c82311 (2026-03-01): fix(terminal): restore full scrollback on reload and fix mobile viewport race
  - 65bb2147 (2026-03-02): fix(jira): handle empty project in JQL builder
  - d913421c (2026-03-03): fix(jira): merge status_map with defaults so empty {} doesn't break transitions
  - a89e864b (2026-03-03): fix: production readiness audit — harden PTY, drones, server, and frontend

## f05e1f1a — feat(terminal): add SerializeAddon and Export button

- Date: 2026-02-28
- Subject capability overlap: terminal
- Corrective follow-ups within four days: 5
  - b0117d1d (2026-03-01): fix(detection): fix plan detection, dedup queen banners, hide empty Custom Rule
  - 95c82311 (2026-03-01): fix(terminal): restore full scrollback on reload and fix mobile viewport race
  - 65bb2147 (2026-03-02): fix(jira): handle empty project in JQL builder
  - d913421c (2026-03-03): fix(jira): merge status_map with defaults so empty {} doesn't break transitions
  - a89e864b (2026-03-03): fix: production readiness audit — harden PTY, drones, server, and frontend

## f93e77d9 — feat(terminal): add custom link provider for file path detection

- Date: 2026-02-28
- Subject capability overlap: terminal, providers
- Corrective follow-ups within four days: 5
  - b0117d1d (2026-03-01): fix(detection): fix plan detection, dedup queen banners, hide empty Custom Rule
  - 95c82311 (2026-03-01): fix(terminal): restore full scrollback on reload and fix mobile viewport race
  - 65bb2147 (2026-03-02): fix(jira): handle empty project in JQL builder
  - d913421c (2026-03-03): fix(jira): merge status_map with defaults so empty {} doesn't break transitions
  - a89e864b (2026-03-03): fix: production readiness audit — harden PTY, drones, server, and frontend

## bd375bb5 — feat: add toggle tooltips, actionable empty states, keyboard shortcut help modal

- Date: 2026-03-29
- Subject capability overlap: terminal, settings, ui_ux
- Corrective follow-ups within four days: 5
  - 6524beab (2026-03-30): fix(terminal): close WS on subscriber drop to prevent frozen terminal
  - 146719a8 (2026-03-30): fix: prevent browser Ctrl+L/D when terminal has focus
  - 0437930e (2026-04-01): fix: exempt /ws and /ws/terminal from session auth middleware
  - e970ee1f (2026-04-02): fix(ui): improve mobile terminal readability and action button layout
  - e11d2662 (2026-04-02): fix: prevent terminal reset on agent spawn output bursts
