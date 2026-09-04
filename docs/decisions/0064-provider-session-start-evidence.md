# ADR 0064: Scoped provider session-start evidence

Status: Accepted implementation design under the approved maturity program.

Recovery uses provider lifecycle evidence, not a live PID or visible prompt.
Claude's [hook reference](https://code.claude.com/docs/en/hooks#sessionstart),
checked 2026-09-04, distinguishes startup, resume, clear, compact and fork.
SessionStart supports command or MCP-tool hooks; HTTP support is not assumed.

The domain accepts normalized New/Resumed evidence only for a matching engine
session and recovery attempt. Exact resume must retain the selected identity.
New evidence settles only an already-authorized Fresh attempt; otherwise it
reports unexpected context. Clear, compact, fork and unknown lifecycle events
do not settle startup recovery. Duplicate or obsolete evidence cannot reopen it.

Payloads are not self-authenticating. Integration must bind a process-scoped
capability, bound input size and callback lifetime, reject replaced sessions,
and persist conversation identity atomically with the current worker binding.
A durable worker credential alone does not identify the originating process.
Preserve existing user/provider settings and command grants. Never forward
transcript contents, paths, titles or prompts or emit new conversation context.

Explicit missing-context evidence remains necessary for Continue-to-Fresh.
Exit, timeout, transport/auth failure and missing callback are not absence.
Interactive conversation switching after startup is a separate lifecycle; it
must update future resumption without reopening a completed recovery operation.

Only the domain rule is implemented in this checkpoint. Callback transport,
authentication, durable reconciliation, missing-context detection and provider
acceptance remain open. This ADR does not install hooks or complete P2.
