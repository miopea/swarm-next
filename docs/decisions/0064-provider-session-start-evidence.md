# ADR 0064: Scoped provider session-start evidence

Status: Accepted implementation design under the approved maturity program.

Recovery uses provider lifecycle evidence, not a live PID or visible prompt.
Claude's [hook reference](https://code.claude.com/docs/en/hooks#sessionstart),
checked 2026-09-04, distinguishes startup, resume, clear, compact and fork.
SessionStart supports command or MCP-tool hooks; HTTP support is not assumed.

The domain accepts normalized New/Resumed evidence only for a matching engine
session and recovery attempt. Exact resume must retain the selected identity.
New evidence settles only an already-authorized Fresh attempt; otherwise it
reports unexpected context. Clear, compact, fork and unknown lifecycle events
do not settle startup recovery. Duplicate or obsolete evidence cannot reopen it.

Payloads are not self-authenticating. Integration must bind a process-scoped
capability, bound input size and callback lifetime, reject replaced sessions,
and persist conversation identity atomically with the current worker binding.
A durable worker credential alone does not identify the originating process.
Preserve existing user/provider settings and command grants. Never forward
transcript contents, paths, titles or prompts or emit new conversation context.

Explicit missing-context evidence remains necessary for Continue-to-Fresh.
Exit, timeout, transport/auth failure and missing callback are not absence.
Interactive conversation switching after startup is a separate lifecycle; it
must update future resumption without reopening a completed recovery operation.

Engine protocol 12 adds a process-capability startup observation request over
the existing private IPC channel. Capability debug output is redacted. The engine
checks the session's liveness and capability gate before retaining evidence.
Protocol 11 terminal control remains supported, but cannot receive this request.

The command helper checks the target engine protocol before sending, refuses
unknown versions, reads at most 64 KiB, and shares a three-second deadline across
stdin and IPC. It emits no stdout or diagnostic payloads and does not retry.
The helper reads the stdin descriptor directly with bounded polling, avoiding an
uncancelable background stdin read or changes to inherited descriptor flags.
The existing registry mutex spans spawn through insertion; callback lookup uses
the same mutex, so it cannot observe the child before registration. This ordering
does not guarantee delivery before the helper deadline under a stalled spawn.
Session summaries now expose the optional retained observation without capability
material. Evidence survives repeated reads and does not itself establish current
liveness or authorize changing the worker's durable default.
Schema 127 records one startup receipt per worker in the session-binding
transaction, including the originally selected conversation. New bindings replace
the receipt; old sessions are not backfilled from a potentially newer selection.
Explicit operator selection cancels pending evidence even when selecting the same
ID, so A-to-B-to-A changes cannot be undone by delayed startup callbacks.
The API's existing lifecycle-locked binding reconciliation consumes accepted
engine observations. Persistence checks the active session, provider and unchanged
selection, applies the domain recovery result and commits the receipt, resulting
pin and activity event together. Repeated observations do not rewrite settled
results. Only engine-owned authenticated attempts are eligible for rehydration;
impossible ladder positions are rejected. No task input is replayed.
The protocol-12 engine generates a separate private startup-settings overlay for
future Claude starts with Swarm MCP configuration. It merges existing grants first,
then appends an idempotent SessionStart command hook using the current engine
executable (shell-quoted), including when there are no grants. Existing hooks,
permissions and explicit hook-disable settings survive. Input and output settings
are capped at 1 MiB; replacement uses a private temporary file and atomic rename.
Failure preserves base settings, not a stale overlay. No provider/user settings
file is edited in place and this development work installs nothing on live workers.
Outcome presentation distinguishes persisted startup results from attempts.

Interactive selection uses a separate bounded state machine. Claude's documented
SessionEnd(reason=resume) names the conversation being left by interactive /resume;
the next SessionStart(source=resume) names the resumed one. A matching end arms one
transition, and a subsequent authenticated resume advances the process-local
selection revision without replacing immutable startup evidence. A mismatched end
or unpaired different start does not authorize a pin update. Revisions order
accepted engine evidence, not undocumented provider timestamps. Old-to-new-to-old
selection is legitimate when each transition has its own matched boundary.
The 2026-09-04 hook reference documents a default 1.5-second SessionEnd timeout;
transport integration must fit that bound or explicitly configure it, never rely
on eventual retries. Missing/reordered lifecycle evidence remains unconfirmed, not
proof that no switch occurred. End transport, revised selection persistence and
actual provider ordering acceptance remain open, along with complete fallback.
The state machine and parser alone do not complete /resume integration or P2.
