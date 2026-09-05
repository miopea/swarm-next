# ADR 0078: Explicit conversation choice survives a worker binding

Status: Accepted implementation design under the approved recovery scope.

An operator-selected conversation remains the default across worker sleep,
maintenance and API replacement. A newer transcript is not authority to replace
that choice. Successful restoration of an arbitrary old pin is not proof of
operator intent either.

Persistence retains one explicit choice per worker, scoped to provider,
workspace and conversation. Only an explicit default command or an authenticated,
paired provider selection records it, in the same transaction as the default.
No historical choice is invented during migration. Existing session revisions
continue to fence late evidence; the durable choice does not bypass them.

On a new binding, startup revision one can confirm this choice only after a
settled exact Restored outcome names the same conversation as the current saved
default and the scoped durable choice. Pending, canceled, suspended, mismatched,
continued or fresh outcomes do not qualify. Later paired selections retain their
existing revision-based confirmation. Workspace moves clear the choice. A newer
explicit choice replaces the previous one; no unbounded history is added.

This is evidence for suppressing contradictory drift warnings, not permission to
switch conversations or infer answers. Test restart, database reopen, migration,
manual fences, wrong identity, failure rollback and stale bindings. Live acceptance
uses only the disposable demo; existing workers' intent is not backfilled.
