# Central support runtime — inactive integration slice

This process owns a separate support database. It has no Hive execution router,
terminal-host connection, worker credentials, or access to customer Hive databases.
See ADR 0080 for the approved full workflow and activation gate.

Configuration:

- `SWARM_SUPPORT_DATABASE`: explicit path to the separate database (required).
- `SWARM_SUPPORT_CAPACITY`: positive maximum conversation count (required).
- `SWARM_SUPPORT_LISTEN`: defaults to loopback `127.0.0.1:4281`.
- `SWARM_SUPPORT_ADMIN_TOKEN`: optional, deployment-provisioned private credential
  of at least 32 bytes. Never distribute it to Hives. Omission disables private
  routes; invalid configuration fails without printing the credential.

Implemented routes:

- Public `POST /api/support/v1/submissions`: explicit email-contact report,
  stable submission UUID, exact-retry receipt, conflicting-payload refusal.
- Optional authenticated `GET /api/ops/admin/conversations` and `/{id}`:
  BFG Admin v1 private list/thread contract with bounded, revision-aware history.

All responses use `Cache-Control: no-store`. Requests share eight admission slots,
a 128 KiB request-body limit, and a 15-second deadline. Blocking persistence retains
its admission slot after a client timeout until it finishes; uncertain submission
outcomes must retry the exact original key and content. Saturation refuses new work
without deleting conversations. A supplied email is contact data, not verified
account ownership. SIGTERM/Ctrl-C initiate graceful shutdown.

This is not publicly activated. Deployment needs operator-approved hosting,
TLS/proxy and intake-abuse policy, source registration, dedicated credentials,
backup/recovery configuration, and paired acceptance. Shared admission is bounded,
but does not guarantee private readers priority over public traffic.

Still required: Hive durable outbox/UI, diagnostic and attachment workflows,
verified inbound email and loop prevention, persisted message roles and authors,
triage/status changes, linked task completion, and operator-approved original-channel
reply delivery. Schema v1 contains customer intake only; do not insert provider or
operator messages and mislabel them as customer content. The current thread response
explicitly disables AI drafting and replies. It does not claim source-alerts,
attachments, email delivery, or a recipient-visible in-app inbox.
