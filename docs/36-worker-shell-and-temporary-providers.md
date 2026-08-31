# Worker shell and temporary alternate-provider workers

Two items return to the worker right-click menu. Both existed in legacy Swarm and
neither survived into swarm-next. Scoped with the operator on 2026-08-26.

They are one document because they share a menu and nothing else. The shell is
deliberately *not* a worker; the temporary provider deliberately *is* one. Most of
the design work is keeping those two facts from bleeding into each other.

## A. Open a shell

A detached scratch terminal that starts in the worker's workspace and cwd. It is
**not** a worker session, and that is the whole design.

Swarm's worker lifecycle assumes an agent at the far end of a pty: briefing
delivery, MCP credential scoping, and `provider_activity` classification all read
terminal output to decide whether someone is working, resting, or blocked. A bash
prompt answers none of those questions. Attached as a worker session it would
classify as permanently unknown, and — the concrete harm — could make a sleeping
worker read as awake in the roster.

So the shell:

- does not appear in the worker roster and has no presence indicator
- is not classified by `provider_activity`
- does not affect whether its worker is awake or asleep
- holds no assignment and writes nothing to the board

It borrows only the worker's *workspace path* as its starting directory.

### Reach and gating — decided, not defaulted

**No gate.** Anyone holding the operator token can already create a worker that
runs arbitrary code, so a shell exposes no capability the token did not already
carry. It makes it direct rather than new.

Reach is the full home directory, because `swarm-terminal-host` dropped
`ProtectHome=read-only` in 0.8.17 — see `packaging/systemd-user/swarm-terminal-host.service.in`,
whose comment block records why. Confining the shell to workspace roots was
considered and rejected as theatre: without a namespace the shell can `cd` out, so
the restriction would be a convention that reads as a boundary. The honest options
were "same reach as the worker beside it" or "a real systemd namespace", and the
first was chosen.

**This is worth re-reading if the threat model ever changes.** The reasoning
depends entirely on the operator token being the only credential and on Swarm not
being exposed beyond the host.

## B. Spawn a temporary worker on another provider

A menu item spawns a **temporary worker** in the same workspace, running a
different provider. It is a throwaway sibling, not a second session bolted onto the
worker you right-clicked.

This matters: running two providers under one worker would break the
one-session-per-worker assumption that sleep/wake, briefing delivery and MCP
credential scoping all rely on. A sibling costs none of that.

### It is a real worker row, flagged temporary

The operator chose **full tools** — a temporary worker can transition tasks, file
new ones, and record deployments like any other.

That decision forces the storage shape. Anything with board access must stay
attributable, so a temporary worker gets an ordinary worker row carrying an
ephemeral flag, not an anonymous session. Otherwise its writes outlive it pointing
at an author that never existed — the stale-record shape with the author deleted.

The flag pays for itself at promotion: **Adopt into the Hive** clears it and asks
for a name. Nothing migrates.

### Lifetime — explicit, never automatic

Two menu actions on the temporary worker itself:

- **Adopt into the Hive** — clears the ephemeral flag, prompts for a name, and it
  becomes an ordinary worker
- **Release** — dismisses it

Named *Release* rather than *Kill* on purpose. A temporary worker with full tools
may have written to the durable record, and those records survive it either way;
"kill" implies they do not. Nothing auto-dismisses on idle, because idle detection
for the alpha providers is exactly the part we trust least.

### Context: brief plus a handoff note

A temporary worker starts with the parent's task brief **and** a short handoff note
the parent writes at spawn time. **Swarm wakes the parent first and waits for the
note.**

A Claude transcript cannot be replayed into Codex — different session formats, auth
and state directories — so cross-provider context has to be portable text. The task
brief already is exactly that, and is what any worker reads at pickup.

**Recorded concern, operator decided otherwise, proceeding as instructed:** the
common reason to want a second model is that the first one is *stuck*, which is
precisely when it cannot write you a note. Waking a wedged parent waits and then
fails. The implementation therefore needs a timeout policy, which the interview did
not settle — see open questions.

## Providers

All four ship. **Claude Code and Codex are supported; Gemini, Grok and OpenCode are
labelled alpha in the UI.**

The alpha label is pointed at the real risk. Launching another CLI is trivial;
knowing whether it is *working or waiting for you* is not, and that judgement is
reverse-engineered from TUI output that changes without warning.

### What already exists — verified in the tree

- `ProviderKind` (`crates/swarm-domain/src/workers.rs:109`) — today `ClaudeCode`
  and `Codex`
- `ProviderCommand { executable, arguments, working_directory }` and the
  `ProviderTerminalAdapter` trait (`crates/swarm-terminal/src/provider.rs:23,39`),
  with `ClaudeCodeAdapter` and `CodexAdapter`
- release probing via `provider_release`, and `executable_in_path` detection
  (`crates/swarm-terminal-host/src/lib.rs:314`)
- the worker-creation API already accepts `{"provider":"codex"}`

So Codex is wired end to end. For it, this feature is UI plus the temporary-worker
lifecycle and nothing else.

### What each new provider costs

1. a `ProviderTerminalAdapter` impl — launch args and working directory
2. a release probe and a PATH detection entry
3. **arms in three classifiers** — `idle_prompt`, `idle_footer` and
   `background_work_signal` in `crates/swarm-terminal/src/provider_activity.rs`

Item 3 is the expensive one and the reason for the alpha label. `idle_prompt` for
Claude is `❯` and for Codex is `›`; these are literal glyph matches against a
redrawn TUI. Getting one wrong does not fail loudly — it makes a worker look busy
forever, or look resting while it is mid-turn, which is how a delivery gets
deferred indefinitely.

Adding a `ProviderKind` variant is mechanically large but **compiler-enforced**:
20 non-test sites across 8 crates must gain arms before it builds, and
`provider_activity.rs` carries no wildcard arm. Treat the compiler as the
checklist.

**A schema migration IS required, and an earlier draft of this document said the
opposite.** The column is declared
`provider TEXT NOT NULL CHECK (provider IN ('claude_code','codex'))` at
`crates/swarm-persistence/src/lib.rs:3279`. Adding a provider means a forward-only
migration widening that constraint, declared in `RECENT_SCHEMA_STEPS` with
`undo_sql`/`probe_sql` like every other step, and idempotent — the migration tests
rewind `user_version` without rewinding tables.

The claim was wrong because the check was run against `workers.rs` and
`migration.rs` and generalised to the schema without reading `lib.rs`, where the
table is actually defined. It was found by a test that tried to stage a rollback
and was refused by the constraint that supposedly did not exist.

One hardcoded string map at `crates/swarm-persistence/src/migration.rs:1227` also
needs the new arm.

### The rollback hazard this creates

The CHECK constrains WRITES only; SQLite does not re-validate it on read, and a
rollback of the code does not rewind the schema. So the widened constraint stays
in the database and the older build reads a value it cannot parse. `from_str` fails, and the row fails to load — so a worker adopted
on Gemini becomes an unreadable worker row the moment the API rolls back to a
release that predates Gemini.

This is not hypothetical: `packaging/linux/test-package-lifecycle.sh` exercises
API rollback with the terminal host preserved, and the release tooling restores the
previous API on a failed health check. A rollback is a normal event here, not an
emergency.

Decide the behaviour deliberately before shipping an alpha provider that can be
*adopted*: fail the row, or degrade an unknown provider to a readable placeholder
that the UI shows as unsupported. The second is strongly preferable — one
unreadable row should not be able to take down the roster — but it is a change to
`from_str` handling at the persistence boundary, not a UI concern, and it should
land **before** the first adoptable alpha provider rather than after.

## Acceptance

- Opening a shell on a worker does not change that worker's presence, and sleeping
  the worker while a shell is open leaves the shell running and the worker asleep
- A shell starts in the worker's workspace cwd
- A temporary worker appears distinctly as temporary and offers Adopt and Release
- A temporary worker's board writes remain attributable after it is released
- Adopting preserves the worker's session and history — it is a flag change, not a
  re-creation
- A temporary worker receives the parent's brief and handoff note in its opening
  context
- Alpha providers are labelled as such at the point of choosing them, not only in
  documentation
- An ablation shows each assertion fails when its mechanism is removed

## Settled, 2026-08-31

All five were put to the operator together. Answers, with the reasoning that
produced them:

1. **A stuck parent proceeds on the brief alone, and the record says so.** A
   parent that never sends its handoff note must not be able to block the
   escape hatch reached for because it is stuck — that is the case the feature
   exists for. The spawn succeeds, the temporary worker starts with less
   context, and the absent handoff is recorded rather than papered over. The
   timeout itself is not yet chosen; anything in the tens of seconds is
   defensible and it is reversible, so it should not hold the work up.

2. **A shell DOES survive an API restart, and the assumption here was wrong.**
   Established by reading rather than asking: `open_worker_shell` issues
   `HostRequest::StartShell`, so a shell is a terminal-host session, and the
   host is a separate service that deliberately survives an API reload — the
   comment at `crates/swarm-api/src/worker_runtime.rs:380` says so in as many
   words.

   What does not survive is the API's KNOWLEDGE of it. There is no shell table
   and no durable record, so after a restart the process is still running, still
   holding a workspace, and no longer reachable from the UI. That is an orphaned
   PTY per API restart, not a shell that died — a leak rather than a loss, and
   the opposite failure from the one assumed.

3. **Naming is auto-generated and not yet specified.** Deliberately left: it is
   reversible, invisible until the first temporary worker exists, and nothing
   else waits on it.

4. **A temporary worker MAY spawn temporary workers, two levels deep.** The
   operator chose this over a flat one-level rule: a spike that needs its own
   spike is real. Two levels bounds the worst case to four processes on a
   four-core box, which is the constraint that actually bites here.

   **Handback follows the chain and release cascades.** Each temporary worker
   is adopted by the worker that made it — the only party holding its context,
   which is what makes a handback note mean anything. Releasing a parent
   releases everything beneath it, so no temporary worker can outlive the
   reason it exists. That is how a temporary worker becomes permanent by
   accident.

5. **An unknown value degrades to a readable placeholder, and it lands BEFORE
   the first use.** See the hazard above; the operator confirmed both the
   remedy and the timing.

   **AND IT IS NO LONGER ONLY ABOUT PROVIDER.** Schema 110 widened the CHECK on
   `tasks.state` to admit `abandoned`, which arms exactly the same hazard for
   task state: `crates/swarm-persistence/src/lib.rs:5062` raises
   `FromSqlConversionFailure` on an unparseable state, and task listings collect
   into `Result<Vec<_>>` — so one row a rolled-back build cannot read fails the
   WHOLE listing, not just that row. Rollback is routine: the release tooling
   restores the previous API automatically on a failed health check.

   Nothing is abandoned today, so the hazard is latent and the first use arms
   it. That is why the timing answer was "before anything is abandoned" rather
   than "with the first alpha provider".

## Related

- `05-terminal-session-model.md` — the session model the shell deliberately sits
  outside of
- `packaging/systemd-user/swarm-terminal-host.service.in` — why the host has no
  `ProtectHome`
