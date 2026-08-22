# Open product questions

Status: **All questions answered — dogfooding set closed 2026-08-22**

The primary operator accepted the recommendations below on 2026-08-10. The
remaining questions should be answered by side-by-side dogfooding rather than
by rebuilding legacy behavior in advance.

## Resolved for implementation

1. **First provider: Claude Code.** It exercises the current primary Swarm
   workflow. Codex follows after the terminal/session contract passes recovery
   and soak tests. Its adapter now preserves the provider boundary: Codex owns
   its thread identifier and recovers the latest cwd-scoped thread with
   `codex resume --last`; Swarm does not manufacture a Claude-style UUID.
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
6. **Provider authority: one immutable backend per Apiary.** Jira-backed ships
   first and every Hive talks to Jira directly. Native later owns a complete
   provider-work protocol; backend conversion is rejected. Swarm-generated
   coordination tasks are distinct: Keeper is canonical and members poll its
   ordered feed in every Apiary.
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
- Live terminal sessions survive browser and API restarts. Worker profiles also
  recover Claude conversation context after stop, crash, or reboot: exact
  session identity for new profiles and workspace `--continue` for migrated
  profiles whose exact identity is not known.
- Provider-native permission policy owns tool approvals.
- No production history import in the first slice; legacy remains readable in
  the old application.
- Desktop and Android Chrome/Edge PWA are required dogfood surfaces. Desktop
  terminal recovery remains the first implementation gate.

## Answered by dogfooding

Answered by the primary operator on 2026-08-22, after running Swarm as the
daily driver and installing it on a second machine. These were deliberately not
answered in advance; each one is now answered from use rather than from
planning.

3. **Durable workers earn their place, and what they own is a repository.** A
   worker is a long-lived identity tied to one repository, carrying context
   across days and sessions. Provider-native subagents are within-session
   fan-out. They are different jobs and both stay — a subagent cannot be woken
   next week to continue what it was doing.

4. **No groups.** Thirty-one workers on the roster and search plus ordering has
   been enough. A grouping is a taxonomy to maintain and a second place for a
   worker to be filed wrongly, and neither organisation nor bulk action has
   been missed in practice.

5. **The coordination set is complete**: finding, blocker, handoff, decision,
   and task-state change. Nothing has been missing through heavy use. Worth
   holding to, because adding an event type is adding a way for two agents to
   talk to each other, which is precisely what Legacy got wrong.

6. **A recorded deployment is the whole verification policy.** It is a fact in a
   table rather than an opinion, which is why the coordinator can act on it
   without a model call. Anything beyond it — judging whether the work did what
   the task asked — is the noisy model judgment this question existed to avoid.
   Not even a deterministic second check for now.

7. **Live Jira synchronisation matters now.** The developers share a Jira board,
   so two Hives will contend for the same tickets, and the accepted Apiary
   ownership and claim model has something real to be validated against. This is
   the one answer here that implies work rather than closing something: it says
   the claim model is about to be exercised for the first time by people other
   than its author.

8. **Legacy is retired when every live workflow has run here first.** Jira
   intake, email replies and deployments all running on Swarm for a period with
   Legacy idle rather than merely available. Not a fixed window: idle-in-fact is
   the evidence, and a duration with Legacy still quietly serving something
   would not be.
