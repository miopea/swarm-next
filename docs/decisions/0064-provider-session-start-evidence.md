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

Every Claude recovery launch carries its engine-owned attempt, including an
ordinary exact resume and a direct native continuation. Attempt recording is not
conditional on the missing-exact fallback probe: otherwise normal SessionStart
evidence has no recovery identity for API reconciliation. Explicitly requested
new conversations remain outside the recovery ladder. No extra probe is added
for direct continuation, and an inconclusive exact probe still retains the exact
attempt. This does not claim context restoration before the provider callback.

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
The protocol-13 engine generates a separate private startup-settings overlay for
future Claude starts with Swarm MCP configuration. It merges existing grants first,
then appends an idempotent SessionStart command hook using the current engine
executable (shell-quoted), including when there are no grants. Existing hooks,
permissions and explicit hook-disable settings survive. Input and output settings
are capped at 1 MiB; replacement uses a private temporary file and atomic rename.
Failure preserves base settings, not a stale overlay. No provider/user settings
file is edited in place and this development work installs nothing on live workers.
Outcome presentation distinguishes persisted startup results from attempts.
Because providers invoke that absolute command after launch, release pruning keeps
the release directory of the live host process even after `host-current` advances
to an identical engine build. The old release becomes removable only after the
host actually restarts. A mapped `(deleted)` executable is insufficient: the host
would keep running while every later lifecycle callback failed to execute.

Live development deployment on September 5 reproduced that failure under the
systemd mount sandbox: reading another same-user process's `/proc/PID/exe`
failed while the owned engine socket remained readable. Cleanup therefore first
queries the running engine's `host_version` through `swarmctl status`, with a
three-second coreutils `timeout` deadline, and protects that release independently
of current, previous, and host-current links. Missing, failed, timed-out, or unsafe
release identity defers all release deletion. The `/proc` check is supplemental,
never evidence that an unreadable process no longer owns files. Subsequent cleanup
can reclaim obsolete releases once running identity is confirmed; no retry loop
or background task is added. This does not repair an already-deleted executable
mapping or establish conversation intent from a successful exact resume.

Overlay creation also requires the engine's own helper path to be absolute and
name an existing executable regular file. A deleted mapping or invalid path
cannot publish a broken command or substitute another release's binary. Failure
retains the existing base-settings fallback and unconfirmed recovery semantics;
an old overlay is not selected. A later valid launch can regenerate the overlay.

Interactive selection uses a separate bounded state machine. Claude's documented
SessionEnd(reason=resume) names the conversation being left by interactive /resume;
the next SessionStart(source=resume) names the resumed one. A matching end arms one
transition, and a subsequent authenticated resume advances the process-local
selection revision without replacing immutable startup evidence. A mismatched end
or unpaired different start does not authorize a pin update. Revisions order
accepted engine evidence, not undocumented provider timestamps. Old-to-new-to-old
selection is legitimate when each transition has its own matched boundary.
Protocol 13 adds ProviderResumeEnd and optional selected-conversation revisions in
session summaries. The engine validates the same live-child capability boundary.
Future worker settings include a resume-only SessionEnd hook. Its helper shares
the bounded stdin/IPC implementation but has a one-second overall deadline, within
the provider's documented default 1.5-second end-hook budget. Both helper modes
preflight protocol 13 and refuse older/unknown versions before sending secrets.
Missing/reordered lifecycle evidence remains unconfirmed, not proof that no switch
occurred. Revised selection persistence and actual provider ordering acceptance
remain open, along with complete fallback; P2 is not complete.

Durable interactive selection uses a monotonic per-session revision. Explicit
operator pin changes must first fence the engine selection stream, then commit
that fence and pin under the existing API lifecycle lock. A fence advances the
engine revision without moving its live conversation and cancels an incomplete
resume boundary. Only later paired transitions can supersede that manual choice.
Unfenced/manual persistence callers suspend automatic following for that binding;
they must never assume their last observed engine revision is a sufficient fence.
New session binding resets tracking. Protocol 14 adds an engine selection-fence
request on private IPC. Both lifecycle helpers now preflight exactly version 14.
The API consumes paired selections under its existing lifecycle lock and fences
explicit choices under that same lock. A current durable receipt is required
before requesting a fence; older bindings without a receipt remain manual-only.
An unavailable or incompatible host does not prevent saving an explicit choice:
automatic following is suspended for that binding instead. Persistence failures
remain errors, not evidence that following is safe. The API reports whether
following was retained. Actual provider ordering and Linux end-to-end acceptance
remain required; this integration is not a completed recovery acceptance gate.

Recovery attention clears after a later engine selection is confirmed against
the exact durable revision, pin, and current unsuspended binding. This bounded
projection (at most 256 candidates) is separate from immutable startup outcomes.
Manual fencing alone cannot clear attention. The existing session response carries
the confirmation without another poll; the terminal keeps earlier startup results
in Session details, not as an ongoing warning after a confirmed switch.

The legacy conversation-drift diagnostic also honors that same confirmed-selection
projection. It reads engine selections with a bounded two-second request and
revalidates them against persistence, then matches the current profile's active
session and saved conversation. A matching confirmed default skips transcript
recency scanning; a newer file cannot overrule the chosen conversation. Missing,
offline, suspended, replaced, or mismatched evidence does not acquire this status.
The existing Current wire state means no unresolved drift, preserving older
browser counts/cards. Remaining timestamp warnings request review, not claim
proven context loss. This does not confirm defaults for sleeping workers or
replace the provider's native resume behavior.

Engine evidence accepted while a provider was alive remains eligible if that
provider exits before the API reads it. Reconciliation applies the retained
startup/selection evidence before releasing missing worker bindings; process exit
does not erase an already-authenticated conversation identity. Persistence still
requires the exact current binding and honors manual-selection fences. A replaced
or already-released binding cannot update the new session's default. This does
not accept new callbacks from dead providers or infer restoration from exit.

Protocol 15 adds StopRetained. It revokes provider lifecycle admission and stops
the child, but leaves the bounded engine session entry available with a pending
release marker. The API saves its final evidence before the existing Stop command
removes the entry. After API interruption, ordinary binding reconciliation saves
that same retained evidence and finishes cleanup. No new timer, history store,
or unbounded pending queue is introduced; entries count against the registry cap.
Persistence failure must leave the entry retained, not acknowledge saved context.

The API stop adapter owns compatibility with protocol 11-14: save their available
pre-stop evidence and then use legacy Stop, reporting the weaker protection in
diagnostics. This cannot guarantee capture of a switch racing that legacy Stop.
Remove this compatibility branch when the supported engine floor reaches 15.
Unknown future protocols are not assumed to implement this handshake. This
change does not turn abrupt process termination into graceful provider shutdown.

Protocol 16 adds the engine-owned final continuation successor described in
ADR 0068; lifecycle helpers now preflight that version. The retained-stop
handshake is unchanged for versions 15 and 16, and the older 11-14 compatibility
branch still provides only its documented weaker pre-stop snapshot protection.
