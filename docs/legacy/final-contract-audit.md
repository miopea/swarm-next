# Legacy final product contract audit

Status: **Reviewed against the final legacy tree**

## Why this exists

Legacy Swarm's final README describes a broad, mature product, but a README is
not executable evidence. This audit checks the promises that matter to an
operator against the final implementation owners and their tests. It does not
declare every passing unit test a product guarantee: a claim is **held** only
when the final tree has a clear owner, meaningful boundary tests, and a narrow
enough promise for those tests to support.

Verdicts:

- **Held** — the final code and tests defend the important operator outcome.
- **Held with limits** — the outcome exists, but the README is broader than the
  evidence or the mechanism remained heuristic.
- **Partial** — useful pieces exist, but they do not establish the whole claim.
- **Contradicted** — the final tree explicitly narrows or rejects the claim.

This is a requirements filter for Swarm Next, not a port checklist.

## Operator journeys

| Journey | Legacy promise | Final implementation owner | Executable evidence | Verdict | Swarm Next disposition |
| --- | --- | --- | --- | --- | --- |
| Durable terminals and recovery | Managed PTYs survive API/dashboard restarts; reconnecting browsers recover current terminal state. | `src/swarm/pty/holder.py`, `pool.py`, `process.py`; browser reconnect/resync code. | `tests/test_holder.py` covers held-worker lifetime, client removal, liveness, list/reap, versioning, and restart-in-place. `tests/test_reconnect_resync.py` covers immediate panel resync and dropped-frame recovery. | **Held** for holder-owned PTY lifetime and reconnect. It does not prove that every provider conversation can always be recovered after the provider itself exits. | Keep the independent Rust worker engine, retained provider conversation identity, canonical snapshots/history, and explicit reconnect protocol. Never couple worker lifetime to the app/API release. |
| Worker state and operator attention | Workers visibly move among buzzing, resting, awaiting input, sleeping, and failed states so the operator knows when to intervene. | `src/swarm/drones/state_tracker.py`, `src/swarm/db/worker_state_store.py`, `src/swarm/server/state_publisher.py`. | `tests/test_state_tracker.py` exercises active-turn signals, throttling, cleanup, wake behavior, and task promotion. `tests/test_state_tracker_first_poll.py` protects first-decision evidence. `tests/test_worker_state_persistence.py` covers stale/corrupt state, restart restoration, sleeping persistence, and failure isolation. | **Held with limits.** Persistence and many guards are strong; classification still depends materially on provider-specific terminal text and timing, which produced repeated regressions. | State is a typed composition of process lifecycle, provider events, engagement lease, task ownership, and attention need. Terminal text may support diagnosis but must not be authority or the sole truth. |
| Queen and deterministic automation | Queen coordinates workers while drones handle safe approvals, idle nudges, message pickup, context pressure, resource pressure, task lifecycle, and verification. | `src/swarm/queen/*`; `src/swarm/drones/pilot.py`, `idle_watcher.py`, `inter_worker_watcher.py`, `context_pressure.py`, `pressure.py`, `task_lifecycle.py`, `verifier.py`, `decision_executor.py`. | `tests/test_queen.py` covers bounded headless calls, parsing, locking, assignment, usage, and session rotation. Watcher and verifier suites cover operator-engagement guards, debounce, hysteresis, bounded reopen, shadow mode, per-worker failure isolation, and no-LLM checks. | **Held with limits.** The automation breadth is real, but many actions are terminal injections governed by regex/timers. Provider-native auto approval has removed much of the original drone purpose. | Split deterministic coordination from Queen judgment. Typed, reversible, policy-complete work avoids an LLM call; ambiguity routes to Queen; external effects keep separate authority. Preserve operator-engagement protection and bounded retries. |
| Tasks, assignment, and coordination | Durable tasks carry lifecycle, history, dependencies, attachments, email origin, worker ownership, and Queen/worker coordination. | `src/swarm/server/task_manager.py`, `task_coordinator.py`, `src/swarm/tasks/*`, `src/swarm/mcp/queen_tools.py`. | `tests/test_task_lifecycle_invariants.py` defends single-active-task, activation/demotion, blocked state, reconciliation, and start timestamps. `tests/test_tasks.py` covers persistence, history, unassignment, email parsing, task types, and workflow templates. `tests/test_queen_tools.py` enforces Queen-only authority for reassignment, interruption, completion, prompting, and learning. | **Held** for the local task record and major lifecycle invariants. **Partial** for broad inter-worker coordination: broadcast and terminal injection caused context and recipient failures. | Keep typed task state, durable ownership, discussion/evidence, and role-scoped commands. Remove fleet broadcast. Operator and Queen may steer workers; worker outcomes return durably to their sender and Queen. |
| Jira work | Jira can be discovered, imported, mapped to Swarm workflow, assigned, commented, transitioned, refreshed, and reconciled two ways. | `src/swarm/integrations/jira.py`, `jira_workflow.py`, `src/swarm/server/jira_service.py`. | `tests/test_jira.py` covers OAuth, JQL, mapping, issue conversion, assignment, comments, attachments, refresh, ADF extraction, and sync. `tests/test_jira_export_reconcile.py` covers export/retry/reconcile behavior. | **Held with limits.** The two-way implementation is substantial; tests principally defend adapters and sync rules, not every real Jira workflow or delivery ambiguity. | Jira remains canonical for Jira-backed work. Use typed cursors/outboxes, explicit workflow mapping, confirmed claims, visible freshness, and retry-safe reconciliation. Never infer success from an uncertain write. |
| Email intake and completion | Outlook inbox messages, inline images, and attachments become tasks; completion produces a reviewed nontechnical reply without silently sending it. | `src/swarm/server/email_service.py`, Outlook import routes, `src/swarm/mcp/handlers/_email.py`. | `tests/test_outlook_import.py` covers normalized listing, separate and merged imports, and partial failures. `tests/test_email_service.py` covers saved attachments, fetched/CID images, normalized content, formatted replies, completion drafts, and failure notification. `tests/test_mcp_draft_email.py` covers permission, validation, draft creation, and failure. | **Held with limits.** Import and draft mechanics are real; safe completion still depends on Graph access, content review, and delivery state outside the local test boundary. | Keep explicit selection/merge, preserved source identity and attachments, editable task fields on import, and a reviewed reply outbox. Never auto-send merely because a task entered Done. |
| Mobile/PWA and notifications | The dashboard installs as a PWA, works well on mobile, reconnects, shares content, badges attention, and notifies the operator. | `src/swarm/web/routes/pwa.py`, dashboard browser code, notification bus/backends. | `tests/test_sw_cache_leak.py` proves the final kill-switch service worker unregisters itself and deletes caches rather than providing offline caching. `tests/test_notification_bus.py` covers routing, debounce, concurrency, and backend tolerance; `tests/test_external_notifications.py` covers authenticated external notifications. | **Contradicted** for the broad offline-PWA claim: the final service worker is deliberately a cache-removal kill switch after the browser-memory incident. **Held with limits** for installability, share intake, reconnect, and notifications. | Treat mobile as a first-class responsive web experience with durable trusted sessions, explicit reconnect, full task/terminal controls, and policy-driven notifications. Add offline behavior only with bounded storage and browser-process soak evidence. |
| Resources, updates, and development mode | Swarm attributes machine pressure, suspends workers safely, updates without losing work, and can reload a development working copy quickly. | `src/swarm/resources/monitor.py`, `src/swarm/server/resource_monitor.py`, pressure manager, holder restart/version protocol, source-tree state. | `tests/test_resource_monitor.py` covers memory/load/PSI/swap parsing, process-tree RSS, D-state descendants, pressure classification, and snapshots. `tests/test_pressure.py` covers suspend/resume and hysteresis. `tests/test_holder.py::TestRestartInPlace` and `tests/test_source_tree_state.py` cover holder/source lifecycle pieces. | **Held** for Linux resource evidence and pressure policy pieces. **Partial** for the README's broad update/dev-mode promise: no single final acceptance suite proves every release path preserves all active work. | Show owned process-tree and machine evidence. Keep app/API hot reload separate from worker-engine replacement, state risk plainly, and require live preservation tests for both paths. |
| Authentication, security, and provider breadth | Remote Swarm is protected by password/session/passkey controls and supports Claude, Gemini, and Codex workers. | Auth/session/passkey/MCP middleware; `src/swarm/providers/*`. | `tests/test_auth_session.py`, `test_password.py`, `test_passkeys.py`, and `test_mcp_auth.py` cover cookies, hashes, credentials, loopback/token boundaries, and worker MCP configuration. The final README itself labels Gemini and Codex experimental; `src/swarm/providers/gemini.py` calls Gemini a stub with unvalidated patterns. | **Held with limits** for tested auth primitives, not a complete hosted threat-model proof. **Contradicted** if the opening multi-provider sentence is read as equal production support; Claude alone is claimed production-ready. | Preserve authenticated browser/API boundaries, scoped secrets, audit, and provider-owned permissions. Treat each provider as independently accepted only after real interactive, recovery, mobile, and upgrade proof. |

## Requirements promoted into Swarm Next

The audit promotes outcomes, not mechanisms:

1. **Worker lifetime is independent of the web/API release.** Provider process,
   conversation identity, terminal history, and task ownership survive app/API
   deployment; worker-engine replacement is a separate, explicit risk surface.
2. **Worker state has typed evidence.** Process state, provider activity,
   operator engagement, task lifecycle, and attention need remain distinct and
   explainable.
3. **Operator engagement blocks automated injection.** Viewing alone is not an
   engagement lease; input or a pending operator decision is. Automation defers
   while the lease is active.
4. **Deterministic rules do deterministic work.** Every rule has an event
   identity, precondition revision, idempotency key, retry bound, typed outcome,
   visible reason, and durable audit record.
5. **Queen owns ambiguity, not polling.** Queen chooses among plausible workers,
   priorities, or blocker interpretations. She does not burn a model call to
   refresh cursors, wake an already assigned worker, or apply a complete policy.
6. **External systems keep their own authority.** Jira writes, email replies,
   deployments, and other effects use confirmed outboxes and cannot be marked
   delivered from an ambiguous response.
7. **Tasks are the durable coordination surface.** Broadcast is not a substitute
   for ownership. Outcomes and blockers return through role-scoped durable
   channels.
8. **Mobile is full product, not an offline claim.** Responsive layout, terminal
   interaction, reconnect, trusted session, task operations, and notifications
   are required; offline caching is optional and must be bounded.
9. **Resource numbers identify their owner.** Machine, API, worker engine,
   provider process, and browser measurements are not collapsed into one
   misleading “terminal memory” value.
10. **Provider support is earned independently.** A shared interface does not
    make provider state detection, recovery, shortcuts, or permission behavior
    equivalent.

## What not to port

- Regex approval clicking as the normal permission system.
- Fleet-wide worker broadcasts.
- Terminal text as task authority or the sole worker-state source.
- Speculative delivery without recipient identity and cancellation proof.
- An offline/service-worker promise without storage and browser-process bounds.
- A settings matrix of tuning knobs before a current operator journey requires
  each one.
- A generic “multi-provider” badge that hides unequal acceptance evidence.

## Next coordinator rule selected by this audit

The next useful rule is **stale owned-work escalation**, not a generic idle
nudge. When a task is assigned to a loaded worker and has durable evidence of
no progress beyond a policy window, the coordinator records one revision-bound
attention event. It must:

- do nothing while the operator is engaged with that worker;
- distinguish sleeping (unloaded), resting (loaded and idle), awaiting input,
  actively working, and disconnected;
- never infer staleness only from terminal text;
- avoid waking or messaging a worker when delivery or state is uncertain;
- escalate to Queen only when choosing the response requires judgment;
- expose the reason, evidence, age, action, and avoided/used Queen call in the
  operator UI;
- cancel automatically when task ownership, task revision, worker session, or
  progress evidence changes.

This keeps the valuable legacy idle-watcher outcome—work should not disappear—
without recreating repeated terminal nudges or spending a Queen call on a
fully specified state check.
