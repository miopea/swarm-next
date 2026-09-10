# Apiary onboarding validation handoff — September 10

This is a validation build, **not release acceptance**. The operator approved
pushing it for another worker to validate. Do not cut a release from this document.

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
