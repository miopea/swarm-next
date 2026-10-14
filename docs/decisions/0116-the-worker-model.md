# 0116 — The worker model: one model for a Hive's repository workers

- Status: Accepted
- Date: 2026-10-14
- Decided by: the operator, on 2026-10-14.
  - Decision 01a137d8: "Worker model next".
  - Decision 01a13bc9, answered in a worker's terminal:
    - "Workers only; Queen keeps Sonnet (Recommended)";
    - "No, one model for all (Recommended)".
- Builds on: ADR 0113. Worker model is a setting a Keeper can require only once
  a member enforces it, and this is what enforces it.
- Task 01a1376b. The design note is on that task (note 20200).

## Context

"Worker model" was in the operator's first policy set, as "the fleet's cost
control". The only model Swarm chose was Queen's: `ensure_queen_model_default`
writes `"sonnet"` into her settings file when it names no model. Repository
workers ran on Claude Code's own default, and nothing on screen showed or set a
worker's model.

The path for a model already existed. Before every Claude start, the API writes
`<worker>.settings.json` beside the worker's MCP config. The terminal host reads
`model` from that file at each launch (`worker_model_beside`), including the
fresh-recovery fallback, and passes it as `--model`. That host code has been in
every release since v1.11.0, so every member in the field can relay a model
without a protocol change or a worker engine update.

## Decision

### 1. One model for every repository worker

`WorkerModel` is `default`, `opus`, `sonnet` or `haiku`.
- `default` is how Hives behaved before this setting: Swarm names no model, and
  Claude Code uses its own. It is every Hive's starting point.
- The others are Claude Code's own aliases, so the setting follows its current
  model mapping instead of pinning a model id that will be retired.
- It covers Claude Code repository workers, Scout included. Queen keeps her own
  Sonnet default and is never moved by it (decision 01a13bc9). Codex and the
  alpha providers name models their own way and are not affected. The
  operator's ad hoc terminals are not workers and are not affected.
- There are no per-worker exceptions (decision 01a13bc9).

It is set in Settings > Workers > Worker model.

### 2. Where it is enforced: the file the host launches from

`AgentBridge::apply_worker_model` runs in the start path after the grants
writer, on every Claude start. The two share the settings file, and each owns
only its own key.
- When the Hive names a model, it is written as the worker's `model`. It
  replaces a model somebody put there by hand, because the ruling is one model
  for all.
- When the Hive is back on `default`, Swarm removes only a model it wrote
  itself. It remembers what it wrote in `worker_model_launches.written`, so a
  hand edit made while the Hive named no model is left alone.
- The pure rule is `launch_model_change`.

### 3. When it takes effect: at each worker's next start

A running session keeps the model it started with. Nothing is restarted for the
setting, because ADR 0113 rules out remote restarts and the operator does not
want workers restarted before they are ready.

So that "in effect" means "started on it", the model each worker's current
session started with is recorded:
- It is read back from the settings file after the write, never assumed from
  what was meant to be written.
- It is written after the session binds, keyed by that session.
- When the host's own continuation recovery replaces the session, the successor
  inherits the record, because the host relaunched from the same file.

A running repository worker whose current session has no record, or a record
for another model, is short of the Hive's model. A session from before this
release counts as short, rather than being assumed to comply. The settings
card says how many workers are short, and that each one switches when it next
starts.

### 4. The Keeper and the member

The Keeper requires the model as `worker.model` (ADR 0113 addendum). The member
applies it where the model is read at each start, keeps its own choice
underneath, and refuses a local change with 409 `set_by_apiary`.

A member that has applied the requirement but still has workers running another
model reports how many in its health report (`workers_off_model`). That field is
optional, so older releases ignore it. The Keeper's roster shows "N workers not
yet on <model>", which is not pushed, since each one switches by itself.

What the workers actually spend stays visible in the Keeper's existing usage
panel, by Hive and by model. That includes an operator's mid-session `/model`
switch, which Swarm cannot prevent.

## Consequences

- Schema 201 adds `worker_model_settings` and `worker_model_launches`.
- A Hive that changes its model still runs the old one on every live worker
  until each restarts. The card and the roster say so rather than restarting
  anything.
- A model hand-edited into a repository worker's settings file is overwritten
  while the Hive names a model, and is not restored afterwards.
- If the model cannot be written, the worker still starts. Its start is
  recorded with whatever the file names, so it shows as short of the model
  rather than as complying.
