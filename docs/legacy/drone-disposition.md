# Legacy drone disposition

## Conclusion

Legacy drones are obsolete as a **separate terminal-reading automation persona**.
They are not evidence that background coordination is obsolete. Swarm keeps
the valuable outcomes by splitting them among typed application services, the
deterministic coordinator, worker health policy, and Queen.

This distinction matters because the file-aware ledger associates 264 commits
with drones, but those sets overlap broad files and releases. Only 83 have a
drone-classified subject; 95 of the 264 are fixes and 62 are release commits.
Commit volume therefore measures churn and coupling as much as product value.

## Why the old mechanism is obsolete

### Provider-native approval removed its original daily job

The early drone watched terminal text, recognized prompts, and clicked or typed
answers so workers could continue unattended. Provider-native automatic approval
now handles ordinary coding-tool permissions directly. Recreating a second
prompt interpreter would duplicate provider behavior while possessing less
structured context.

### Terminal text never became a safe authority boundary

The approval chain repeatedly widened from verbs to effects and then found new
holes: SQL mutation and privilege grants (`814876ca`, `19730e44`), compound
commands (`dccf03c8`), credential reads and outbound payloads (`70f9d10b`),
ordinary-work false denials (`9173b1ed`), moved credential paths and real network
tools (`5f16ad5f`), and a measured remaining auto-approval leak (`d6476e99`).
Legacy ultimately separated hard gates from human-review brakes in `26796719`.
The stable lesson is that typed effects, not terminal substrings, own authority.

### The drone shared too many responsibilities

The same subsystem accumulated approvals, idle nudges, sleeping transitions,
revival, pressure response, `/compact`, message pickup, task dispatch, handoff
generation, goals, verification sweeps, and Jira-divergence checks. That made an
apparently small behavior depend on prompt parsing, worker classification,
timers, configuration, task state, terminal writes, and Queen messaging. The
history records concurrency races, stale completion, duplicate proposals,
wrong-recipient preparation, revive loops, ineffective settings, and nudges
firing while work was already active.

### A named mini-agent hides the real owner

Calling all background behavior a drone blurs whether an action is a database
transition, a resource policy, a provider input, an external effect, or a
judgment call. Swarm instead gives each operation one domain owner and one
authority contract. The implementation can be tested without pretending that a
general-purpose agent made the decision.

## What survives in Swarm

| Legacy outcome | Next owner | Rule |
| --- | --- | --- |
| Expire leases, retry outboxes, reconcile known state | Deterministic coordinator | Typed, idempotent, bounded, and no LLM call. |
| Wake an already assigned worker and deliver a durable brief | Coordinator plus worker engine | Exact worker and revision; engagement and provider-question guards; uncertain delivery never replays blindly. |
| Detect loaded, resting, sleeping, stuck, or pressured workers | Worker health policy | Runtime/provider events, hysteresis, measured evidence, and a visible reason. |
| Run repository-owned completion checks | Bounded verification job | Run inside the owning repository; return evidence rather than declaring success from ancestry alone. |
| Interpret ambiguity, prioritize, reassign, or recommend an answer | Queen | LLM judgment only where policy-complete deterministic work cannot decide. |
| Approve trust, credential, destructive, purchase, or external-message effects | Operator or explicit authority policy | Never inferred from terminal text or confidence. |

The coordinator is intentionally not presented as another bee with a terminal or
conversation. It is application machinery below Queen. If a future background
action needs free-form interpretation, hidden terminal parsing, or expanding
authority, it has crossed out of the coordinator and must return to Queen or the
operator.

## Evidence-based classification

- **Obsolete constraint:** clicking routine provider permission prompts because
  providers lacked native automatic approval.
- **Already prevented:** unrelated writes during operator engagement,
  wrong-recipient dispatch, unattributed PTY input, and replay of uncertain
  delivery at the shared Next boundaries.
- **Relevant redesign:** health monitoring, bounded revival, task delivery,
  outbox reconciliation, and verification scheduling.
- **Optional opportunity:** repository-owned daily verification after Ring 1
  demonstrates repeated false completion across repositories.
- **Unresolved evidence:** any Queen-answerable provider prompt class. Ring 1
  remains notify/recommend-only until exact prompt identity, expiry, answer,
  read-back, and recovery are proven.

## Product test

A background behavior belongs in the deterministic coordinator only when all of
these are true:

1. the target, revision, preconditions, and allowed effect are typed;
2. the action is idempotent or has a durable at-most-once boundary;
3. failure and uncertainty return visible evidence without optimistic success;
4. operator engagement and an open provider question cause a recoverable hold;
5. no natural-language interpretation or new authority is required.

If any condition is false, the work belongs with Queen or the operator—not with
a revived version of Legacy drones.
