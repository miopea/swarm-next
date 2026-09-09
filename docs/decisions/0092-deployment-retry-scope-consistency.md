# ADR 0092: Deployment retries preserve the saved completion scope

Status: Accepted correctness decision under approved QUEEN-01 and QUEUE-01.

## Evidence

Deployment receipts deduplicate by task, environment and reference. The insert
ignores an existing identity, but immediate Awaiting Release settlement used the
new call's whole-task argument rather than checking the saved receipt. A retry
could therefore close work whose durable evidence still described partial work.
The regression reproduces this without a provider or live Hive mutation.

## Decision

Persistence compares the saved receipt's whole-task scope inside the same
transaction, before settlement or exemption changes. Identical scope retries
retain their existing identity. A conflict in either direction rolls back and
returns an explicit conflict; it never upgrades or downgrades historical evidence.
A later genuinely different verified delivery uses a distinct reference, retaining
the old partial receipt. Agents receive the reason; HTTP exposes status 409 and
`deployment_scope_conflict`. No new permission, schema, timer or queue is added.

This does not change the operator's whole-deployment settlement policy or introduce
a reviewer veto. It makes the command agree with the evidence it actually saved.

## Verification

Cover exact retries, both conflicting directions, unchanged task/activity on
refusal, no automatic completion from partial evidence, and recovery through a
new whole-task receipt. Retain existing deployment/prerequisite and no-deployment
settlement tests. Live demo deployment acceptance remains a separate gate.
