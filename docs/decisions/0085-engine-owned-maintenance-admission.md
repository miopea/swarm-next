# ADR 0085: Engine-owned maintenance admission

Status: Accepted implementation direction under the approved update-safety scope;
not implemented or live-validated. The release gate remains open.

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
