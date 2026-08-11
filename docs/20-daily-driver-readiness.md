# Daily-driver readiness: legacy outcomes and Next product cut

Status: **Accepted implementation sequence**

This comparison uses the current legacy UI, routes, tests, and operator
observations as evidence. It does not treat legacy parity as the goal. Swarm
Next keeps an outcome only when it contributes to reliable daily work.

## Current position

Swarm Next has crossed the infrastructure gate: terminal state is bounded and
host-owned, browser reload recovery is canonical, API updates preserve active
PTYs, and the sidecar advances only at zero sessions. The missing work is now
mostly product operability rather than process survival.

| Daily outcome | Legacy Swarm | Swarm Next now | Decision |
|---|---|---|---|
| Resume after reload | Feature-rich but observed redraw and reconnect failures | Canonical recovery verified with three live workers in an actual browser | Complete baseline; continue multi-day soak |
| Update during work | Sidecar separation existed | Live proof preserves the same host PID, session, and interactive PTY | Complete |
| Know who is working | Configured names, roles, groups, status labels | Durable named worker profiles retain workspace, provider, order, and session binding | Complete dogfood baseline; add richer attention states |
| Coordinate with Queen | Interactive Queen PTY plus separate headless automation | One durable, always-active Queen survives API/browser updates | Complete interactive baseline; defer autonomous conductor policy |
| Move between workers | Keyboard switching and cached terminals, but redraw was flaky | Repeated Queen/worker switching preserves exact session identity on desktop and mobile | Complete baseline; continue fast-output soak |
| Plan and dispatch work | Rich task modal, priorities, dependencies, imports, proposals | Explicit lifecycle, editing, priority, assignment, focused state drops, and bounded activity history | Add operator-controlled ordering; defer broad integrations |
| See changes without refreshing | Broad WebSocket event stream | Typed, authenticated, resumable control-room invalidation feed | Complete; keep Refresh as recovery |
| Manage workers quickly | Launch, kill, revive, groups, bulk actions | Start by path and stop | Add start/revive/stop on profiles; defer groups until measured |
| Use direct interactions | Shortcuts, command palette, drag/drop imports | Basic buttons and forms | Add shortcuts, accessible menus, task reorder and focused drop targets |
| Configure the product | Large multi-tab editor with overlapping concerns | No settings surface | Add a compact settings workspace for workers, runtime, appearance, and diagnostics |
| Handle attention | Proposals, messages, activity, notifications | None | Later: one decision inbox, not multiple legacy subsystems |
| Diagnose failures | Large logs/config surface | Browser, API, database, terminal, provider, and integration health with sanitized preview/copy | Complete foundation; add submission transport when selected |

## First dogfood cut

### A. Durable worker roster and Queen

A worker is a stable profile; a terminal session is one process incarnation.
Profiles own name, role, provider, workspace, order, and autostart policy. A
profile may point to at most one running session. Session history remains
immutable when that profile starts again.

The Queen is a singleton profile with role `queen`, stable name `Queen`, and
autostart enabled. The API supervisor reconciles her profile against the
terminal host after startup and after an exit. API restart must attach to the
existing Queen session rather than create a duplicate. A failed launch is a
visible unhealthy state with Retry; it is never hidden in a restart loop.

### B. Terminal correctness and switching

All running terminal surfaces remain mounted in a stable deck. Selecting a
worker changes visibility, not ownership. Acceptance covers repeated switching,
reload, API restart, sidecar reconnect, resize, wrapping, scroll position,
alternate-screen applications, ANSI color, and fast output while hidden.

### C. Live control room

One authenticated, resumable event stream carries roster, session, task, and
health changes. Events invalidate typed snapshots; they do not mutate separate
client-side copies of domain state. Manual Refresh remains a recovery action.

### D. Task ergonomics

Task detail and editing, description, priority, assignment, activity history,
and focused state drag/drop are implemented. Add operator-controlled ordering
without weakening explicit state transitions. Email, Jira, WYSIWYG, pipelines,
and broad automation wait for dogfood evidence.

### E. Operator controls

Add keyboard navigation, an accessible action menu for each worker/task, a
command palette, and settings for roster, runtime, appearance, and diagnostics.
Context menus supplement visible controls; they never become the only path.

## Explicit non-ports

The first dogfood cut does not port routine approval drones, regex approval
rules, broad headless Queen automation, pipelines, standing loops, playbook
synthesis, Jira/Outlook synchronization, groups, or bulk actions. Each may
return only through a measured operator journey.

## Promotion gate

Ring 1 begins when the operator can:

1. open Swarm Next and find Queen already live;
2. create named workers once and start or revive them without retyping paths;
3. switch and reload without terminal corruption;
4. create, edit, prioritize, assign, and complete ordinary coding tasks;
5. receive live roster/task changes without manual refresh;
6. update API/browser code without interrupting workers;
7. identify which subsystem failed and submit a useful sanitized report.

Ring 2 requires a multi-day soak with legacy still available as an independent
fallback. Missing outcomes become evidence; they do not automatically become
legacy ports.
