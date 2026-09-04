# Ops Console MCP integration

Status: isolated implementation; not enabled or deployed. See ADR 0060.

The separate `POST /mcp/ops` endpoint exposes only `ops_submit_ticket` and
`ops_ticket_progress`. It uses the existing rmcp streamable HTTP transport in
stateless JSON mode. It never accepts worker, Queen, OAuth-connection or browser
operator credentials as a fallback. The existing `/mcp` endpoint is unchanged.

## Configuration and revocation

Set `SWARM_OPS_INTEGRATIONS_FILE` to an absolute path on the API host. With no
path configured, the endpoint returns 404. The private file contains up to eight
integration entries and must be at most 64 KiB:

```json
{
  "integrations": [
    {
      "integration_id": "bfg-ops-console",
      "token_sha256": "<64 hexadecimal characters: SHA-256 of the bearer credential>",
      "disabled": false,
      "bindings": [
        { "app_id": "<registered console app ID>", "workspace": "<approved absolute repository path>" }
      ]
    }
  ]
}
```

Generate at least 32 random bytes for the credential. Keep the original only in
the console's secret manager; the console stores its secret reference. Swarm's
file contains only the digest. Provision the file with owner-only permissions
and replace it atomically, never by rewriting a live file in place. Verify every
workspace against the intended repository before configuring it.

The file is read on every request. Disable or remove an entry to revoke it on
the next request, or atomically replace its digest to rotate the credential.
In-flight authenticated work may finish. Duplicate integration IDs, duplicate
digests, malformed mappings, unreadable files and oversized configuration fail
closed. Credentials are compared using constant-time digest equality; neither
credentials nor private configuration appear in responses or logs.

No runtime credentials or app mappings have been provisioned by this change.

## Commands and receipts

Both tools require `integration_id` to match the authenticated credential. This
prevents a rotated or incorrectly provisioned credential from creating the same
source key under a different integration. The caller cannot choose its identity.

`ops_submit_ticket` accepts `integration_id`, `app_id`, `request_id`, `conversation_id`, reviewed
`title`, reviewed `description`, and `priority` (`low`, `normal`, `high`, `urgent`).
Identifiers are at most 128 ASCII identity characters; titles are at most 240
UTF-8 bytes and descriptions at most 64,000 UTF-8 bytes. Workspace and actor
identity are always derived from authenticated configuration.

The normalized command, source key and task creation are one transaction.
Successful structured content is `{"ok":true,"ticket":{"task_id":"...",
"replayed":false}}`. Retrying the identical integration/app/request command
returns the original task with `replayed:true`, including after task removal.
Changing the command under that key returns `submission_conflict`; the caller
must not invent a new request ID merely to bypass an uncertain earlier result.

Successful submission creates an inert draft. It cannot assign a worker, start
a terminal, transition a task or send a customer message.

`ops_ticket_progress` accepts `integration_id`, `app_id` and `request_id`. It returns selected task
facts, at most 50 recent activity records and at most 50 deployment records.
Each page has an explicit `truncated` flag. It excludes the task description,
workspace and worker/session identifiers. Deployment records retain environment,
reference, deployed/recorded times and `delivers_whole_task`.

`deployment_recorded` means some deployment evidence exists. A partial deployment
does not mean the whole request shipped. Task closure, closure on evidence,
unverifiable closure and deployment scope must remain distinct in the console.
Never generate a shipped update from task state alone, and never silently infer
absence of older evidence when a page is truncated.

## Failures and limits

Tool refusals include `ok:false`, a stable code and `retryable`. Codes are
`invalid_command`, `submission_conflict`, `not_found`, and `unavailable`.
Only `unavailable` is retryable. Malformed tool arguments and unknown tools are
MCP errors. Internal persistence diagnostics are not exposed.

Missing/incorrect/revoked credentials return 401; unavailable configuration or
storage returns 503; an untrusted host returns 403. The endpoint admits at most
two concurrent requests, returns 429 on saturation, and limits request bodies
to 512 KiB with a 15-second body-read deadline. A blocking database job retains
its admission permit even if the caller disconnects. The console owns durable,
bounded retry/reconciliation; no retry loop runs inside the MCP endpoint.

The API-only release must preserve the terminal host and running worker sessions.
Protocol/auth tests use isolated fixtures and never submit production tickets.
