# Component and engineering-principles audit

Status: **Active guardrail**

## Purpose

Swarm Next is being built quickly with AI, but speed does not excuse accidental
coupling. This audit turns DRY, YAGNI, KISS, SOLID, WET, separation of concerns
(SoC), principle of least astonishment (POLA), and single level of abstraction
(SLAP) into concrete repository decisions rather than slogans.

`WASP` does not have one established meaning in this repository or in common
software-design references. It must be defined by the team before it becomes an
enforceable rule. Inventing a meaning would itself violate POLA.

## How the principles work together

- **DRY applies to knowledge and behavioral contracts**, not merely similar JSX.
  Repeated state rules, accessibility behavior, or option lists should have one
  owner.
- **WET is an intentional waiting rule.** Two small pieces of similar markup may
  remain separate until their reasons to change are proven to be the same.
- **YAGNI and KISS prevent speculative frameworks.** Swarm does not need a
  general component library or plugin system to share three known behaviors.
- **SOLID and SoC put state with its owner.** Browser persistence, task queries,
  terminal lifecycles, and domain transitions should not accumulate in view
  components or transport handlers.
- **POLA protects the operator contract.** Labels and actions must reflect the
  actual state: Sleeping means unloaded, Wake worker starts it, and Unassigned
  must remain a reversible selection.
- **SLAP keeps orchestration readable.** A function should coordinate named
  operations or implement one operation, not alternate between both levels.

## Current evidence

### P0 — repeated behavior that can drift

1. Task-board controls existed independently in the desktop rail and mobile
   board. Filter values, sort choices, Jira links, and manual synchronization
   could diverge. They now use one `TaskBoardControls` component.
2. Task and worker reordering independently owned dragged-id, insertion-target,
   cleanup, and reorder calculations. They now use one `useReorderDrag` behavior
   hook while retaining domain-specific row rendering.
3. Worker-rail sizing and local persistence lived in the root application
   component. It now has a focused `useWorkerRailWidth` owner.
4. Task cards inferred assignment from a matching repository even after the API
   durably returned work to the unassigned queue. That compatibility guess was
   removed: repository affinity can guide a recommendation but cannot replace
   explicit assignment truth.
5. Apiary connection and invitation handoffs now share one URL-fragment codec,
   one accessible link-entry component, and one file-fallback component. The
   signed payload remains domain-specific, but encoding, size bounds, labels,
   paste behavior, and fallback drag behavior cannot drift between Keeper and
   member flows.
6. Notification permission, subscription persistence, and startup repair remain
   owned by `NotificationController`. Settings only renders its typed state;
   it does not infer browser capability from the server's aggregate device
   count.

These abstractions are justified by shared behavior, not visual resemblance.

### P1 — oversized presentation responsibilities

- `web/src/App.tsx` remains the browser composition root and still mixes
  commands, navigation, mobile overlays, and layout rendering. The typed
  snapshot, saved-session restoration, live-feed invalidation, recent event
  evidence, and all aggregate replacement/clear behavior now have one owner in
  `useControlRoomModel`. Cancelled effects cannot apply stale room state, and
  locking cannot leave workspace or Jira-link state behind while hiding the
  rest of the room.
- `web/src/tasks/TaskBoard.tsx` has dropped below 400 lines. Query/filter/sort
  is a pure tested `taskBoardModel`; collection composition remains in the
  board; and `TaskCard` now owns its vertical editing, assignment, activity,
  Jira discussion, email resolution, action-menu, and drag behavior. Metadata
  and assignment remain focused child components rather than a generic card
  framework.
- `web/src/settings/ApiarySettings.tsx` previously combined identity editing,
  both bootstrap handoffs, policy review, Jira readiness, membership,
  Stewardship, shared-work rollup, and collapse. Signed handoff controls now
  have one focused owner and Keeper invitation management is a vertical feature
  component that owns candidate loading, identity verification, invitation
  issuance, copy fallback, and operator feedback. Personal-Hive joining remains
  the next vertical extraction; do not replace either workflow with a generic
  settings abstraction.
- `web/src/api.ts` still combines public contract types with every domain HTTP
  operation, but authentication, consistent no-store behavior, typed runtime
  errors, and bounded transient recovery now have one small owner in
  `web/src/api/request.ts`. Domain modules can depend on that helper without
  importing or recreating unrelated contracts. Presence is the first complete
  domain moved onto that boundary: its vocabulary, reads, manual-mode command,
  and device-observation command now live together in `web/src/api/presence.ts`
  while the public barrel remains compatible. Worker discovery, repository
  choices, profile configuration, durable ordering, and start/stop commands now
  likewise share `web/src/api/workers.ts`; terminal-session compatibility
  operations deliberately remain outside that profile-owned boundary. Core task
  vocabulary, bounded activity, ordering, creation, editing, lifecycle, and
  assignment now share `web/src/api/tasks.ts`; Jira synchronization and email
  intake remain integration-owned instead of leaking into the task core.
- `web/src/styles.css` is roughly 1,165 lines in one global
  cascade, making component ownership and mobile regressions harder to see.

Required next extractions are vertical and behavior-led:

1. Continue splitting `TaskCard` only when one of its owned behaviors becomes
   independently complex. Metadata, assignment, query/filter/sort, Jira
   discussion, and email resolution already have focused owners; do not create
   a generic card framework.
2. Extract personal-Hive joining as its own vertical feature component when its
   next behavior lands. Keeper invitation management is already separated, and
   shared cryptographic handoff parsing remains independent of both views.
3. Continue splitting the Jira API contract over the extracted shared request
   helper and proven presence, worker, and core-task slices.
4. CSS moves with extracted feature components after their visual contracts are
   stable. A wholesale CSS-module migration is not justified during alpha.

### P1 — backend adapter concentration

- `crates/swarm-api/src/lib.rs` is roughly 11,600 lines. Routing, state composition,
  task handlers, worker lifecycle, workspace validation, diagnostics, tests, and
  supervisors share one module.
- `crates/swarm-persistence/src/lib.rs` is roughly 3,700 lines although Jira,
  workers, decisions, notifications, dispatches, and outcomes have begun moving
  to focused modules.
- `crates/swarm-domain/src/lib.rs` is roughly 2,800 lines and should be grouped by
  domain vocabulary before cross-Hive work expands it.

The file size is evidence, not the rule. Split only at existing domain and
ownership boundaries. Do not introduce microservices, repository traits per
table, or generic command buses; those would conflict with the modular-monolith
decision and YAGNI.

Recommended sequence:

1. Move API workspace/worker routes behind a `workers` adapter module because
   outside-root approval and terminal-host startup now form a coherent boundary.
2. Move task HTTP handlers behind a `tasks` adapter module while keeping all
   transition rules in `swarm-application` and persistence.
3. Continue extracting persistence by aggregate only when a changed aggregate
   is already under test.
4. Split domain types by the approved architecture modules without changing
   public serialized contracts.

### Closed contract gaps from this pass

- Unassignment has persistence and browser tests proving that stable worker
  ownership clears and a queued brief cannot later reach the former worker.
- Explicit outside-root approval has API and terminal-host tests. An arbitrary
  path must never silently become trusted, and filesystem roots remain blocked.
- Drag insertion cues have interaction assertions for both tasks and workers;
  arrow controls remain the accessible fallback.
- Worker-rail resizing has keyboard and persisted-width tests and remains
  disabled on mobile.
- Desktop and mobile browser proof must still cover the same task filters and Jira
  links now that they share one component.
- Apiary link payloads stay after the URL fragment, are never fetched by a
  third-party relay, preserve signed expiry and exact-Hive binding, and retain
  file import only as a collapsed compatibility path.
- Notification disable no longer unregisters the push service worker. A device
  that was explicitly enabled remembers that intent and repairs a missing
  subscription at startup when browser permission remains granted.

## Definition of a shareable component

A component or hook should be shared when at least one is true:

- it owns the same state transition in two places;
- it implements the same accessibility contract in two places;
- it renders one product concept whose changes must appear everywhere;
- duplicated code has already drifted or caused a defect.

It should remain local when similarity is cosmetic, consumers need materially
different behavior, or the proposed API has more flexibility than current use
cases require.

## Review gate for new work

Every feature pass answers these questions before merge:

1. Which layer owns the new fact or transition?
2. Is the behavior already implemented elsewhere?
3. If repeated, do both copies have the same reason to change?
4. Does the operator-facing label match the actual domain state?
5. Can the main path be read at one level of abstraction?
6. Are failure, recovery, mobile, desktop, and bounded-resource behavior tested?
7. Did the change add a framework or future option that current product behavior
   does not require?

The audit is revisited after the task-board extraction, before Apiary UI work,
and whenever a root browser or Rust adapter module grows by another major
responsibility.
