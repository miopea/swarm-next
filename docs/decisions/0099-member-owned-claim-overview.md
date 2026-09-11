# ADR 0099: Member-owned claim overview

Status: Accepted

## Context

The member Apiary overview calls shared-work, but its local read requires Keeper
authority. Real WSL inspection reproduced HTTP 403 while the other overview
requests succeeded. Membership must not be removed or permissions broadened to
repair this read.

## Decision

Members read their own active claims through the existing outbound authenticated
Keeper transport. GET federation/claims authenticates the node credential and
scopes persistence by Apiary, node, Hive and operator. It returns at most 1000
confirmed or unexpired reserved claims. Released and expired reservations are
excluded. The Keeper's local all-member rollup remains Keeper-only.

The application service owns the persistence operation; the HTTP adapter selects
local or remote transport using durable membership context. Existing transport
deadlines, response bounds and typed errors apply. Reads make no assignments,
renewals, approvals or claim transitions. Unavailable or older Keepers produce a
visible read failure, never a fabricated empty success or a permission fallback.
No new retry loop, migration or member re-enrollment is introduced.

## Acceptance

Prove independent-member isolation, reservation expiry/release, invalid and
expired credential rejection, recovery with a valid credential, HTTP authentication
and no-store responses, and member overview success after no-Jira enrollment.
Verify the live WSL overview after both endpoints run the change.
