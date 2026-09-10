# Apiary onboarding validation handoff — September 10

This is a validation build, **not release acceptance**. The operator approved
pushing it for another worker to validate. Do not cut a release from this document.

## Live gate checkpoint — September 10, approximately 16:49 Eastern

- Candidate `604405a1` includes explicit stopped-sync retry. CI run 34528619897
  completed successfully: web, Linux packaging, Rust security audit, full Rust
  workspace checks/tests, and the release-mode terminal resize gate. Rust job
  duration was 16m26s. Confirmed from the completed GitHub run on September 10.
- Earlier Apiary build `25bafd52` passed full CI run 34526969593.
- Production-dev health is okay, but serves `1.7.0-dev-6ac6c14543de`; its checkout
  is `25bafd52`. Checkout revision alone is not activation evidence.
- WSL localhost:8766 is healthy but reports release `1.7.0`, not this candidate.
- Edge testing still fails before connecting (kernel assets path unavailable).
- Asked the operator whether the other worker owns both in-place deployments
  and live validation, to avoid concurrent deployment. No live update, membership
  removal, credential replacement, or worker restart was performed in this check.

Subsequent read-only check: production now serves
`1.7.0-dev-29fd3f6e05a8-20260910205639-738544`, healthy with no degraded entries.
Remote git ancestry confirms `604405a1` is included in `29fd3f6e`; subsequent
commits are release-gate tests and 1.7.1 notes/formatting. WSL still reports 1.7.0.
This proves production activation of the candidate, not directory convergence or
WSL upgrade acceptance. Deployment was performed outside this task's actions.

Reviewed the subsequent upgrade test in `18427fc7`/`29fd3f6e`: it constructs a
populated **Keeper** database, removes schema 162-164 tables, rewinds to 161, then
reopens and asserts Hive/operator IDs, Apiary association/name, private task and
new profile row. It does not construct an active remote member or assert node
keys/credentials. Treat this as additional Keeper persistence coverage, not proof
of the full existing-member credential or live multi-Hive gate. Latest-main CI
run 34529126794 has now completed successfully for `29fd3f6e` (all four jobs,
Rust 16m5s); `604405a1` CI is also confirmed successful. Both CI watch processes
completed with exit code zero. No test jobs remain running for this gate.

Validate the candidate on both sides after WSL activation; no additional feature
scope is being opened.

## Implemented

- Clearer invitation delivery/retry and preserved confirmed join success.
- Combined explicit policy acceptance and joining when readiness permits it.
- Signed public profiles and bounded, complete member directory exchange.
- Roster projection independent of private Hive identities and permissions.
- Normal member-initiated sync exchanges the directory without changing the
  signed project catalog contract. Old Keeper 404/405 leaves other sync available.
- Public name/email rendering; explicit joining profile preview and default-name
  replacement. Custom names remain unchanged. A profile-save failure prevents
  membership submission and preserves typed edits.
- Temporary Keeper HTTP failures remain retryable, not incompatible versions.

## Verified so far

- Independent three-Hive persistence convergence, rename, departure and private
  task preservation; signature, scope, replay and transaction-failure coverage.
- Actual HTTP join/directory exchange and rename, plus isolated old-endpoint
  fallback. This HTTP test has one member, not two.
- Five domain directory tests; explicit join-profile HTTP authentication/default
  naming test; strict API Clippy and TypeScript checks.
- Combined 39 Keeper/member/settings/profile/join UI tests pass.
- Explicit retry: persistence tests preserve identity/credentials/history, coalesce
  repeated requests, and roll back when the event write fails. Real HTTP recovery
  rejects an unauthenticated retry, then moves the existing stopped member through
  authenticated retry to Current. Five directory/retry UI tests and strict API
  Clippy pass. This is isolated evidence, not verification of the live WSL Hive.

The earlier full persistence run covered schema 162. Later profile/directory
schemas 163/164 have targeted migration and failure/recovery checks, not another
full persistence run. Review migration compatibility before activating a binary.

## Required next checks — no remove/rejoin workaround

1. Upgrade an existing Keeper plus two existing member Hives. Preserve their
   membership, node/Hive/operator IDs, keys, credentials and private work. Confirm
   all three see the complete roster and a member rename reaches the others.
2. Existing members stopped in an incompatible state can request “Retry Apiary
   synchronization.” This queues the existing runner (normally checks every 15
   seconds), retains failure history and credentials, and verifies the connection
   again before reporting success. The HTTP test covers stopped → authenticated
   retry → current without rejoining. Validate the existing WSL instance in place;
   merely updating the build does not automatically clear its stored stopped state.
3. Verify the new “Edit shared profile” control for already joined Hives. It uses
   the ordinary local profile endpoint, does not rename defaults automatically,
   and does not submit an invitation or join. Integrated UI tests cover saving
   and post-save refresh failure; real Keeper/member browser checks remain open.
   Do not ask existing users to rejoin to set their profile.
4. Verify directory freshness/unavailability UI on Member and Settings screens:
   missing/unreadable directory status warns that the list may be incomplete;
   a saved snapshot shows its issue time and an expired-window warning when
   observed expired. Refresh retains the last snapshot on failure, and retry
   refreshes both status and the roster. This does not claim live presence.
5. Browser-check the fictional `join-apiary` harness on narrow and desktop layouts,
   then a new fictional third member through invitation, approval and joining.
   Confirm the outcome is understandable without extra finalization clicks.
6. Test offline/reconnect, response loss, mixed builds, and normal shared task/Jira
   sync. A directory error must not remove private work or silently grant authority.

Edge browser tooling failed before connecting with a kernel-assets path error;
the new profile preview has not had visual acceptance. Do not substitute component
test success for this gate. Production/WSL real memberships were not mutated.

The pushed history also includes earlier native-answer persistence/terminal work.
An application-only update must not silently restart workers to activate it. Check
the normal schema/engine compatibility gate and describe any required engine
transition explicitly. This push is not evidence of deployment or worker continuity.
