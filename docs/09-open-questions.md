# Open product questions

Status: **M0 implementation choices resolved**

The primary operator accepted the recommendations below on 2026-08-10. The
remaining questions should be answered by side-by-side dogfooding rather than
by rebuilding legacy behavior in advance.

## Resolved for implementation

1. **First provider: Claude Code.** It exercises the current primary Swarm
   workflow. Codex follows after the terminal/session contract passes recovery
   and soak tests; the adapter contract is designed against both first.
2. **Terminal retention: time and byte bounds.** Use a small in-memory journal
   for fast resume plus bounded on-disk history with time and byte eviction.
   Set exact values from a representative output trace and soak test, not
   intuition.
3. **First usable milestone: terminal-first vertical slice.** Prove two-worker
   switching, browser reload, API restart survival, and bounded memory before
   introducing the task board.
4. **Organization shape: personal Hives with optional Apiary federation.** One
   operator owns one Hive and personal Queen. Keeper owns the Apiary; optional
   Stewards receive explicit Hive, project, and capability scopes.
5. **Membership: exclusive and non-migrating.** A Hive belongs to zero or one
   Apiary, leaves before joining another, and never moves shared tasks between
   Apiaries automatically. A safe sole-Hive collapse is explicit.
6. **Shared authority: one immutable backend per Apiary.** Jira-backed ships
   first. Native later owns a complete distributed task protocol; mixed mode
   and backend conversion are rejected.
7. **Team execution: distributed, not one shared Linux account.** Hives retain
   repositories, provider identity, terminals, and execution nodes. Apiary
   coordinates shared tasks and policy.
8. **Jira: project-scoped distributed synchronization.** Every Hive uses its
   operator Jira identity. Keeper promotes Apiary projects after readiness;
   Hives see and claim shared work without routine Keeper approval.
9. **Primary mobile surface: Android Chrome/Edge PWA.** Mobile includes the
   complete product workflow, provider-aware controls, long-form voice input,
   presence, and actionable push notifications.

## Defaults we can safely start with

- One local operator and one Hive. Apiary identity exists in the model but
  federation UI and distributed coordination follow the local dogfood slice.
- Tasks require title, workspace, state, and optional assignee; richer metadata
  waits for observed need.
- Terminal sessions survive browser and API restarts but not an explicit worker
  stop.
- Provider-native permission policy owns tool approvals.
- No production history import in the first slice; legacy remains readable in
  the old application.
- Desktop and Android Chrome/Edge PWA are required dogfood surfaces. Desktop
  terminal recovery remains the first implementation gate.

## Dogfooding questions

3. Do durable workers remain useful alongside provider-native subagents, and
   which work belongs to each?
4. Are groups needed for organization, bulk action, or neither?
5. Which coordination events are actually required beyond finding, blocker,
   handoff, decision, and task-state change?
6. Which verification policies catch real failures without producing noisy
   model judgment?
7. At which task slice does live Jira synchronization become necessary to
   validate the accepted Apiary ownership and claim model?
8. What side-by-side observation period and migration evidence are sufficient
   to retire the legacy runtime?
