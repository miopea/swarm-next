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
- `SWARM_SUPPORT_OPS_TOKEN`: optional separate ordinary polling credential.
  It cannot equal the private Admin token. Omission disables all Ops floor routes.
- When Ops polling is configured, `SWARM_SUPPORT_ENVIRONMENT` must explicitly be
  `development`, `staging` or `production`, and `SWARM_SUPPORT_MIN_FREE_MIB` must
  be a positive available-space threshold. Optional `SWARM_SUPPORT_BUILD_SHA`
  identifies the deployed revision; omission reports unknown, not a made-up SHA.

Implemented routes:

- Public `POST /api/support/v1/submissions`: explicit email-contact report,
  stable submission UUID, exact-retry receipt, conflicting-payload refusal.
- Optional authenticated `GET /api/ops/admin/conversations` and `/{id}`:
  BFG Admin v1 private list/thread contract with bounded, revision-aware history.
- Optional separately authenticated `GET /api/ops/manifest`, `/health`, `/metrics`
  and `/incidents`: single `swarm-support` source discovery. Ordinary polling
  cannot read conversations. Metrics/incidents are empty until implemented,
  not fabricated telemetry. The manifest advertises only available private reads.

Health commits a content-free, single-row durable database probe and checks free
space on that database's filesystem against the explicit threshold. It does not
certify every database page, a future write, or backup health. Failed read/write
or insufficient space is unhealthy; unavailable disk observation is degraded
(including platforms without the current Unix disk adapter). Schema 2 adds only
the bounded probe row; schema-1 migration preserves original submission encoding
and exact-retry receipts. This is not a customer-content diagnostic export.

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
reply delivery. The current schema contains customer intake only; do not insert provider or
operator messages and mislabel them as customer content. The current thread response
explicitly disables AI drafting and replies. It does not claim source-alerts,
attachments, email delivery, or a recipient-visible in-app inbox.
