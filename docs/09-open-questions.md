# Open product questions

Status: **Narrowed after M0 evidence review**

Only two answers block the walking skeleton. The remaining questions should be
answered by side-by-side dogfooding rather than by rebuilding legacy behavior
in advance.

## Blocking implementation

1. **First provider:** should the first complete adapter target Claude Code or
   Codex? The other provider follows after the terminal/session contract passes
   recovery and soak tests. **Recommendation: Claude Code first**, because it
   exercises the current primary Swarm workflow; design the adapter contract
   against both providers before implementation.
2. **Terminal retention:** how much detached terminal history should remain
   locally available by default: a time window, a byte budget, or both? The
   architecture will enforce both hard memory and disk bounds regardless.
   **Recommendation: both**—a small in-memory journal for fast resume plus a
   bounded on-disk history with time and byte eviction. Set exact values from a
   representative output trace and soak test, not intuition.

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
