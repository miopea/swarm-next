# M1 minimal task workflow

Status: **Implemented and browser-validated foundation**

This slice turns the durable terminal foundation into the first complete work
journey without importing the legacy task-board architecture.

## User outcome

The operator can:

1. create a task with a clear title and allowed workspace;
2. mark the draft ready;
3. launch Claude directly from the task or assign an existing running session;
4. move work through active, blocked, review, and completed states;
5. reload or replace the API without losing the task or assignment.
6. open a bounded activity timeline showing creation, edits, assignment, and
   lifecycle transitions.
7. arrange open work in a durable operator-defined order using drag/drop or
   keyboard- and mobile-accessible controls.

Starting from a ready task creates a new immutable worker session, records a
separate assignment, activates the task, and opens its terminal. A partial
failure remains visible rather than fabricating a completed transition.

## Ownership

- `swarm-domain` owns task identity and permitted state transitions.
- `swarm-persistence` exclusively owns SQLite, forward schema migration,
  transactional task/activity/order writes, integrity checking, and online
  backup.
- `swarm-api` authenticates and validates transport requests, then invokes the
  persistence and terminal boundaries.
- React renders the control room; it does not own task, assignment, worker, or
  terminal lifetime.

The SQLite database defaults to
`~/.local/state/swarm-next/swarm-next.sqlite3`. The packaged API receives the
smallest writable systemd path needed for that state and retains a `0077`
umask.

## Initial task contract

```text
draft -> ready -> active -> review -> completed
                    |         |
                    v         v
                  blocked --> active
```

Ready and active work may become blocked. Blocked work may return to ready or
active. Review may return to active. Skipped transitions fail with conflict
rather than silently rewriting history. Completed tasks cannot be reassigned.

## Design foundation

The diagnostic shell becomes a control-room layout with:

- explicit Tasks and Workers surfaces;
- stable session-derived Claude labels instead of positional worker numbers;
- human task-state language and consistent semantic color;
- a reusable spacing, typography, surface, border, and interaction token set;
- preserved terminal controllers when switching surfaces or workers;
- responsive behavior that keeps primary navigation available on narrow
  screens.

This is the point where product design begins. Future task, decision, message,
and integration surfaces extend this system rather than inventing independent
visual conventions.

## Promotion gates

- domain and persistence transitions are unit tested;
- API create/list/transition paths are integration tested;
- API activity reads are authenticated, bounded, ordered, and served with
  `no-store`; unknown tasks and invalid bounds fail closed;
- API reorder writes require every current open task exactly once, so stale,
  partial, duplicate, cross-Hive, and completed-task requests fail atomically;
- browser component tests cover authentication restore, task creation, and
  lazy activity loading;
- the packaged lifecycle grants only the SQLite state path write access;
- real-browser validation covers creation, editing, assignment, state
  transitions, activity history, durable ordering, desktop/mobile rendering,
  and reload persistence without console errors;
- the two-worker soak gate remains mandatory before M1 is marked complete.
