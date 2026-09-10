# ADR 0085: Engine-owned maintenance admission

Status: Accepted implementation direction under the approved update-safety scope.
The engine-library admission core is implemented and tested. Exact durable source
return records are implemented locally. Native evidence, admission/return receipt
integration, negotiated IPC/package activation and live validation remain
incomplete. The release gate remains open.

## Incident and requirement

On September 7 at 23:38 Eastern, the automatic package reconciler replaced the
worker engine while the operator reports Platform was still processing a stop
request. A concurrent App/API update obscured which operation interrupted work.
The engine census derives activity from a screen, ignores interactive ownership,
and allows new input after the check: existing drain only prevents new sessions.
The package subsequently restarts the service without another protected admission.

An App/API update is not authority to interrupt workers. Automatic engine updates
remain in scope; restricting every update permanently to zero loaded workers is
not completion. Manual interruption remains a separate, explicit operator action.

## Decision

Separate ordinary session-creation drain from maintenance admission. A census is
diagnostic evidence, never a permit to restart a process containing live PTYs.

The engine owns admission serialized with every operator, coordination and
steward input path, and acquisition of interactive control. The package cannot
obtain an admission result, release the protection and later stop those workers.
The engine performs the authorized retained stops under that protection; it does
not hand the package an expiring permission token for a later service restart.
The package may replace the engine only after the exact admitted session set is
stopped and its final context is reconciled. Reads and history inspection remain
available. Acquiring all session input/control guards before checking eligibility
avoids stopping the first workers before discovering an unsafe later worker.
Guard ordering and reader-thread interactions must be pinned by concurrency tests;
do not hold terminal-output locks across process shutdown.

Admission must cover the complete immutable running-session set. New session
creation is already drained. Before any stop, durably record the loaded workers
owed a return. Revalidate membership and session identity at admission. A missing
return record, poisoned lock, unknown provider state or unsupported engine
capability refuses automatic maintenance without stopping workers.

An interactive owner, unsubmitted composer text, unfinished provider turn,
pending accepted submission, or live provider-owned background work prevents
automatic admission. The ordinary Resting classifier intentionally has different
semantics and must remain suitable for routing work; do not overload it into an
execution-completion proof. Input accepted after idle evidence invalidates that
evidence until the corresponding execution is demonstrably settled. A timer or
an arbitrary quiet interval cannot establish completion. Providers lacking that
evidence must expose the limitation rather than advertise safe automatic updates.

If any session refuses, none is stopped and every temporary input/control hold
is released. The owner receives a typed reason, not an apparent successful update.
If a retained stop fails after admission, preserve the exact remaining return
obligations and report partial progress; do not replay operator input or conceal
the failure. Capture final provider selection through the existing retained-stop
boundary before the engine itself exits. Returning a process is not proof of
resuming the intended conversation.

## Compatibility and package ownership

### Explicit package preparation

The package lifecycle owner may stage a validated protocol-changing package via
`prepare-protocol RELEASE_DIR` without draining sessions, contacting the engine,
changing active links, or starting/stopping services. One pending package is
owned by the existing lifecycle lock. A different pending version refuses rather
than replacing a preparation; exact retries preserve the candidate.

Preparation writes a manual-activation hold before the pending-package marker.
Automatic completion refuses that hold even with zero loaded workers. Managed
explicit maintenance must name the exact prepared version; an older request is
consumed without activation. Successful activation removes both markers. These
files carry no authority to stop a worker; the existing explicit maintenance
confirmation and durable return path remain required.

This command is not yet wired into the development build request or exposed as
a working UI action. That integration must advertise preparation separately from
activation, bind consent to the prepared version before stopping workers, and
exercise failure/recovery plus final conversation return. Older installed APIs
write the running version in maintenance requests; they cannot activate a new
manually prepared candidate through that request. Do not weaken exact-target
checking to make an older UI appear compatible. This does not enable automatic
maintenance admission or close OPS-01.

### September 9 implementation checkpoint

The engine library now freezes remote takeover authority, registry membership
and drain cancellation, then acquires every session's stop/control guards before
checking eligibility. Per-session acquisition is non-blocking: an in-flight write
or stop refuses admission rather than waiting behind a blocked PTY. Lock order is
takeover authority, registry, then each session's stop/control guards; no terminal
output guard spans a stop. A live local or remote owner refuses admission.
The supplied return-session set must exactly match all running immutable sessions;
duplicates, omissions and replacements refuse before stopping anything.

Retained stops run while all guards remain held. Failed eligibility releases all
holds without a stop; failed execution returns the exact stopped identities and
the failed identity, leaving subsequent sessions untouched. Successful stop
tombstones reject later input/control effects. Explicit cancellation still uses
its separate stop guard and does not wait for a blocked ordinary writer.

This is a prerequisite, not an enabled updater. Current providers (including
scratch shells) return `ProviderEvidenceUnavailable`; screen activity and startup
hooks cannot authorize maintenance. Positive execution in tests uses fictional
eligibility, not a claim of verified Claude behavior. The library has no production
IPC caller yet. Adding that command requires a protocol bump, so it must land with
the actual native evidence and durable return workflow, not force a migration for
an unusable partial interface. Protocol 16 and ordinary package behavior remain
unchanged. At this first checkpoint the API still had only worker-level revival
intents; the next checkpoint below adds their exact source identities.

Seven focused tests cover all-guards-before-checking, all-or-none refusal,
local/remote ownership, in-flight input, input losing to maintenance, poisoned
state, duplicate/missing/changed return sets, refusal recovery and exact partial
failure. Barriers establish ordering; one receive deadline bounds test failure,
never production eligibility. Linux terminal tests passed (141 in the final
rerun, with the already-passed sustained-output test not repeated and one existing
profiling test ignored); 27 terminal-host tests and strict all-target clippy pass.
No live worker was used, stopped or restarted for this checkpoint.

### Exact durable source-return records

Schema 157 adds at most one exact source-session record per existing revival
promise, within the same 256-worker queue bound. Promise settlement or explicit
cancellation owns removal through a foreign key; elapsed time and source-session
history cleanup cannot erase the record. No terminal text or conversation content
is retained. An old promise is not backfilled from today's active session.

The existing authenticated, drain-required `prepare-return` endpoint now validates
every engine-reported running session against the active worker binding in one
transaction. Unknown/unbound, ended, duplicated or oversized sets refuse the whole
request with a conflict, without stopping sessions or partially recording promises.
Its no-store response retains `recorded_workers` and adds the exact worker/session
pairs and their original source-record timestamps. Retries of the same set retain
those timestamps. Existing explicit engine/provider maintenance recording also
captures actual active source sessions in the same promise transaction.

This is not an admission receipt and cannot authorize a later stop. The future
combined admission owner must retain lifecycle ownership from durable recording
through the engine's protected stop, reconcile final context before replacement,
and preserve failure/return outcomes. IPC/package integration and provider-native
settled execution evidence remain required. Existing revival attempt/settlement
policy is unchanged by this migration; durable reporting after a failed revival
and API replacement remains a separate acceptance gap.

Verification: all 708 persistence tests pass, including populated-schema migration,
backup/restore checks and seven exact-return cases. Nine targeted API tests cover
authenticated preparation, rejection of live unbound sessions, explicit maintenance
and bounded return behavior. Strict all-target persistence/API clippy passes.
The schema and API changes have not been deployed to the development Hive.

### Durable return outcomes

Schema 158 and ADR 0077 now retain an exact attempt before launching a returning
worker. Failed or unconfirmed outcomes survive API replacement and are excluded
from automatic supervisor, Queen and task-dispatch starts. Exclusive lifecycle
ownership, not a timer, identifies abandoned attempts. A confirmed new binding
settles the promise transactionally; old replies cannot clear a newer attempt.
Needs You links the concise unresolved outcome to diagnostic recovery guidance.
This is local implementation, not proof of successful native conversation return
or a completed combined engine-admission/package-update journey.

Explicitly negotiate the new engine capability. An old engine cannot safely
apply the missing admission mechanism to itself. Its loaded-session automatic
replacement must defer; the existing warned manual path can bridge that first
update. The package lifecycle owner removes this compatibility gate after all
supported installed engines implement admission. It must never fall back from
an unsupported or rejected admission to screen-census-based replacement.

Package activation and engine reconciliation must also serialize their install
link mutations. App/API deployment status distinguishes its own result from a
concurrent engine operation; it must not claim preserved workers from unchanged
artifact hashes alone. Engine update outcomes name the initiating path and the
actual stopped/returned session counts, without retaining terminal content.

## Required verification before release acceptance

- A live interactive owner blocks admission and explicit release permits a new
  check. The census guard in eeac1217 covers only this observation, not admission.
- Use synchronization barriers, not sleeps, to inject input/control acquisition
  between eligibility inspection and stop. Either the input wins and maintenance
  refuses, or maintenance wins and input receives a definite non-delivery result.
- A queued submission cannot look idle merely because provider output has not
  advanced; a later unrelated snapshot cannot manufacture completion evidence.
- A real provider-owned background process blocks engine termination even when
  its CLI is at an ordinary prompt. Unknown providers fail closed.
- One unsafe session prevents any partial stop of the otherwise-idle set.
- Failed admission and cancellation release all holds; normal input still works.
- Failed retained stops, lost maintenance responses, API restart and package
  failure preserve bounded return obligations and never repeat a stop blindly.
- Older engines defer without an implicit destructive fallback. Manual consent
  stays explicit and retains its interruption warning.
- Concurrent App/API activation and reconciliation cannot mutate release links
  independently or falsely report worker preservation.
- Exercise real fictional-worker work across update, verify exact provider
  conversation identities and return timing, then run an operator-workload soak.

The original 23:38 Platform misclassification versus race is not yet reconstructed.
These requirements address demonstrated boundary weaknesses without pretending
that the incident's exact last-screen contents were recovered.

## Provider evidence constraint

### Operator-facing restart warnings

The displayed busy-worker census is observational, not maintenance admission.
An empty census must not claim that no work can be lost. Engine confirmations
explain that loaded processes stop, conversation return is attempted, interrupted
commands are not automatically resumed, and unsent input may be lost. A source
fingerprint match is not a claim of byte-identical packaged binaries. Destructive
confirmation initially focuses Cancel; a pending operation cannot be dismissed
through Escape or its backdrop. These UI protections do not implement or relax
the automatic all-session admission gate above.

### Explicit-stop serialization prerequisite

An explicitly authorized single-session stop fences new input/control effects
before terminating the process. It must not wait for the ordinary control guard:
a provider that stops reading can leave a PTY write holding that guard forever.
Stop attempts serialize separately, then acquire child and provider-lifecycle
locks; neither output nor control locks span termination. A successful stop leaves
a session-local tombstone. An effect already in flight reports uncertain delivery
and must not be replayed; an effect refused before it starts reports non-delivery.
Canonical output and final conversation evidence remain readable. Failed stops
permit ordinary recovery without claiming success; successful retries do not
repeat a kill. This is explicit cancellation, not automatic maintenance admission:
the automatic path must still acquire all input/control guards and prove no work
is in flight before stopping any session. A Resting screen grants no such authority.

The Claude hook reference checked September 8 documents `background_tasks` and
`session_crons` on Stop, but also parallel hook execution and Stop hooks that can
continue the conversation. A received Stop callback is therefore not by itself
a committed idle transition. Missing background registry fields are unknown, not
empty. The eventual provider adapter must fence accepted input and account for
hook continuation before presenting a maintenance-safe state. Do not enable
automatic admission by merely adding a Stop callback to the current overlay.

Source: https://code.claude.com/docs/en/hooks (Stop input, Stop decision control,
and parallel hook execution). Installed-provider behavior still needs a fictional
session fixture; documentation is not live acceptance evidence.
