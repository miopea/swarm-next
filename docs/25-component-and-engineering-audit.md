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
  have one focused owner. Keeper invitation management owns candidate loading,
  identity verification, invitation issuance, copy fallback, and operator
  feedback. `PersonalHiveJoin` independently owns connection-card generation,
  invitation preview/import, policy acknowledgement, readiness, and the durable
  prepared-request state. The settings root composes those two vertical
  features without creating a generic federation framework.
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
  intake remain integration-owned instead of leaking into the task core. Jira
  readiness, OAuth entry, project/workflow configuration, bounded issue review,
  task links, comments, sync commands, and reconciliation now share
  `web/src/api/jira.ts`; malformed rolling-update link responses fail closed.
  Email configuration, bounded inbox and message reads, attachment previews,
  multi-source task import, source links, deployment evidence, and the reviewed
  reply outbox now share `web/src/api/email.ts`. Contract tests exercise the
  transport boundary without reading, importing, replying to, or otherwise
  mutating real mail.
- `web/src/styles.css` is roughly 1,165 lines in one global
  cascade, making component ownership and mobile regressions harder to see.

Required next extractions are vertical and behavior-led:

1. Continue splitting `TaskCard` only when one of its owned behaviors becomes
   independently complex. Metadata, assignment, query/filter/sort, Jira
   discussion, and email resolution already have focused owners; do not create
   a generic card framework.
2. Keep Keeper invitation management and personal-Hive joining as separate
   vertical feature owners. The eventual approved outbound join command belongs
   in `PersonalHiveJoin`; shared cryptographic handoff parsing remains
   independent of both views.
3. Presence, workers, core tasks, Jira, and email are now split over the shared
   request helper. Add another domain module only when a remaining behavior is
   independently complex; do not turn the barrel into a directory-per-endpoint
   exercise.
4. CSS moves with extracted feature components after their visual contracts are
   stable. A wholesale CSS-module migration is not justified during alpha.

### P1 — backend adapter concentration

- `crates/swarm-api/src/lib.rs` is still over 11,000 lines. Routing, state
  composition, diagnostics, tests, and supervisors share one module. Worker
  discovery, repository catalog/boundary validation, profile
  creation/editing/order, and start/stop routes now share a focused `workers`
  adapter. Core task list/create/activity/reorder/update/transition/assignment
  routes likewise share a focused `tasks` adapter while lifecycle rules remain
  in `swarm-application` and persistence. Terminal process ownership and
  recovery remain with the root engine composition until that boundary is
  independently extracted. Operator-presence observation/manual overrides and
  device-scoped presentation preferences now have separate focused adapters;
  notification policy, subscription validation/lifecycle, test delivery, and
  the bounded Web Push sender now share the existing `notifications` owner.
  Queen autonomy policy remains separate rather than being coupled merely
  because Settings renders it nearby.
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

1. Keep the extracted worker adapter at the HTTP/profile boundary; do not move
   terminal-host supervision into it merely to shrink the root file.
2. Keep the extracted task adapter at the HTTP boundary. Transition rules,
   activity durability, Jira delivery, and worker coordination remain owned by
   the existing application and persistence layers rather than being duplicated
   in route code.
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
