# ADR 0073: Code without deployment is a reviewable claim

Status: Accepted under approved QUEEN-01.

## Context

The live disposable heartbeat task produced code and passing tests but could
not record a no-deployment claim. The existing guard equated code with a required
deployment. Even Queen could not approve because the claim had been refused.
Local experiments and fixtures can have no deployment in their requested scope.

## Decision

Allow a reported code task to record a bounded, reasoned no-deployment claim.
It remains a claim, not completion evidence accepted by its author. Queen or
the operator may approve after checking task scope and verification evidence.
The deterministic coordinator cannot approve a code exemption or automatically
settle code from its existence alone. Existing routine documentation/no-code
settlement remains unchanged. Missing commit reports still refuse a claim.

Queen guidance checks scope before creating deployment work. Required deployments
remain required; this path neither grants deployment authority nor creates a
deployment record. Normal completion guards, outstanding review requests and
other obligations remain in force. No new queue, timer or schema is introduced.
The API retains the historical `commits_contradict_no_deployment` error identity
for compatibility, but its message now explains the deterministic-approval
restriction. The API boundary owns its removal at the next versioned error-contract
change; clients should not infer that recording a code claim is forbidden.

## Verification

Exercise code claim admission without completion, deterministic refusal, Queen
approval with a basis and subsequent completion. Preserve missing-claim errors,
withdrawal, re-claim, worker-role isolation and routine settlement tests. Live
acceptance additionally requires the demo worker and Queen to use this path;
persistence tests alone do not prove that integration.
