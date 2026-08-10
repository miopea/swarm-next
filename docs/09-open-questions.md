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

## Defaults we can safely start with

- One local operator and one workspace.
- Tasks require title, workspace, state, and optional assignee; richer metadata
  waits for observed need.
- Terminal sessions survive browser and API restarts but not an explicit worker
  stop.
- Provider-native permission policy owns tool approvals.
- No production history import in the first slice; legacy remains readable in
  the old application.
- Desktop browser is the first supported surface; remote and phone use are
  measured during dogfooding.

## Dogfooding questions

3. Do durable workers remain useful alongside provider-native subagents, and
   which work belongs to each?
4. Are groups needed for organization, bulk action, or neither?
5. Which coordination events are actually required beyond finding, blocker,
   handoff, decision, and task-state change?
6. Which verification policies catch real failures without producing noisy
   model judgment?
7. Which integration should be restored first: GitHub, Jira, Outlook, or remote
   access?
8. What side-by-side observation period and migration evidence are sufficient
   to retire the legacy runtime?
