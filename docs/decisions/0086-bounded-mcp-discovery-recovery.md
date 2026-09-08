# ADR 0086: Bounded MCP discovery recovery

## Status

Accepted implementation direction, September 8, under the approved daily-driver
reliability program. Live acceptance remains required.

## Evidence

Platform's September 8 provider transcript records failed MCP initialization:
the stdio bridge could not send its HTTP request to the local API. Subsequent
tool searches returned the stored connection failure. The worker completed
repository work but could not record the outcome in Swarm. The original network
failure's exact cause is unproven; a healthy later API does not repair the
provider's failed connection automatically.

The bridge forwards each request once. Claude's documented automatic reconnect
does not cover local stdio servers. Its `/mcp` panel provides reconnect without
requiring a new conversation. Do not repeat the claim that tools are permanently
fixed at session start or that a full worker restart is the only recovery.

Sources: https://code.claude.com/docs/en/mcp#automatic-reconnection and
https://code.claude.com/docs/en/debug-your-config.

## Decision

The stdio bridge owns a bounded recovery policy for `initialize` and `tools/list`
only: at most six attempts, exponential backoff starting at 500 ms, and a
20-second total deadline including request time. Only connection/timeout failures
and HTTP 502/503/504 are eligible. Authentication, malformed responses and other
HTTP failures are not transient by assumption. Exhaustion produces an explicit
JSON-RPC error; it never supplies an empty successful tool list.

Never automatically replay `tools/call` or other messages, including on a timeout
or lost response. The backend may already have performed a write. This policy
is transport recovery, not permission to restart a provider, replace a session,
alter credentials, send terminal input, or declare provider-visible tools healthy.
Provider reconnect still requires its supported interaction and guarded input.

The deadline and attempts bound resource use; correctness relies on the method
allowlist and successful response, not a presumed quiet interval. No background
retry task survives the request. Verify transient initialization recovery,
non-retried writes/authentication failures, exhaustion and response identity
before promotion. Existing bridge processes require a supported reconnect to
load changed code; do not claim an App/API update alone fixes those processes.
