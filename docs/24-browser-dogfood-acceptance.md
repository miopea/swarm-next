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
