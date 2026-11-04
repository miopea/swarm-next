# 0153 — Store transactions take the write lock when they begin

- Status: Accepted
- Date: 2026-11-04
- Decided by: engineering, within the persistence boundary
  (docs/04-architecture.md). The API's single connection behaves the same, so
  no product behaviour was put to the operator.

## Context

A CI run failed in the graceful delivery test with "database is locked". The
test opens its own store on the database of an API process it started. Just
after the API reported ready, the test moved a task from Draft to Ready, and
the move failed at once. The same test passed 40 times in a row locally, and
again when CI re-ran it.

The store opens transactions as deferred, which is rusqlite's default. A
deferred transaction reads first and asks for the write lock at its first
write. SQLite does not call the busy handler for a lock asked for that way. So
while another connection holds the lock, the write fails straight away, and
the 5 s busy timeout every store connection has is never used. A transaction
that writes first, or begins IMMEDIATE, does wait. A probe on the system's
SQLite 3.45 showed this, with a 2 s timeout (the store bundles its own SQLite,
and the test below reproduces the immediate failure with it):

- Deferred, reading and then writing: locked after 0.00 s.
- Deferred, writing first: locked after the full 2 s timeout.
- IMMEDIATE, reading and then writing: locked after the full 2 s timeout.

Nearly every store move reads before it writes. The crate has 299
`transaction()` calls, tests included. A handful of store methods already ask
for IMMEDIATE, and one says why: to serialize separate store connections.

The API itself has one connection, behind a mutex, so this cannot happen
inside it. It happens when a second connection writes to the same file: a test
beside a running API, a tool, or an operator's own writer. The store already
expects such a writer: its review queue cache is tested against one.

## Decision

The store's connection opens every transaction IMMEDIATE. This is set once,
where the store is built, so both the file and the in-memory store get it,
and so does every transaction added later. A transaction now takes the write
lock when it begins, and waits up to the busy timeout for it like any other
lock.

## Consequences

- A second writer makes a store move wait for the lock instead of failing.
  The move still fails if the lock is held past the busy timeout.
- A transaction that only reads also takes the write lock. In the API, which
  has one connection, that costs nothing. Another connection that wants to
  write waits for it. Transactions are short, as docs/04-architecture.md asks.
- A plain reader on another connection, such as `sqlite3` or a backup, is
  unaffected: in WAL mode a reader never needs the write lock.
- A second store's own transactions take the write lock too, even when they
  only read. So across two processes, each waits behind the other's
  transactions, and one held past the busy timeout fails the other. Before,
  a deferred reader went ahead, and a deferred writer failed at once.

## Validation

- `a_transition_waits_for_another_connection_s_write_lock` (persistence):
  - A second connection holds the write lock for 300 ms while the store moves
    a task from Draft to Ready.
  - The move waits and succeeds.
  - Without this decision it fails at once with "database is locked", code 5,
    the error CI reported.
- The full workspace suite passes with every transaction IMMEDIATE.
