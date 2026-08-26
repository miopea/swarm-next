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

**No schema migration is required.** A provider is stored as a plain string and
parsed back with `ProviderKind::from_str` (`crates/swarm-persistence/src/workers.rs:553`);
there is no CHECK constraint enumerating the valid values. One hardcoded string map
at `crates/swarm-persistence/src/migration.rs:1227` also needs the new arm.

### The rollback hazard this creates

Because the value is parsed rather than constrained, an older build cannot read a
newer provider. `from_str` fails, and the row fails to load — so a worker adopted
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

## Open questions

1. **Wake-the-parent timeout.** How long does Swarm wait for the handoff note, and
   what happens then — proceed with brief only and say so, or fail the spawn? The
   stuck-parent case is the motivating one, so this is not a corner.
2. **Shell lifetime across an API restart.** Assumed not to survive, since it holds
   no durable record. Not confirmed.
3. **Temporary worker naming before adoption.** Auto-generated, and from what.
4. **Whether a temporary worker may itself spawn temporary workers.**
5. **Unknown-provider degradation on rollback** — placeholder or hard failure. See
   the rollback hazard above; this one blocks adopting an alpha provider.

## Related

- `05-terminal-session-model.md` — the session model the shell deliberately sits
  outside of
- `packaging/systemd-user/swarm-terminal-host.service.in` — why the host has no
  `ProtectHome`
