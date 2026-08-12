# ADR-0013: Scoped agent application bridge

Status: Accepted

## Context

Queen and workers need typed ways to plan, assign, and report work before the
unified decision inbox can receive meaningful requests. Legacy Swarm exposed a
large MCP catalog, trusted caller-supplied worker names, and mixed transport,
authorization, and business rules. Reproducing that surface would restore the
same authority ambiguity and coupling Swarm Next is intended to remove.

Claude Code supports explicit MCP configuration at process launch. The official
Rust MCP SDK supplies protocol negotiation and Streamable HTTP handling, so
Swarm does not need to maintain a hand-written JSON-RPC transport.

## Decision

Swarm Next exposes one loopback Streamable HTTP MCP endpoint as an adapter over
shared application services.

- Every durable worker profile receives an independent, revocable bearer
  credential. The database stores only a SHA-256 digest; the plaintext secret is
  kept in a mode-0600 provider configuration under Swarm's state directory.
- Identity and role come only from that credential. Query parameters, tool
  arguments, provider text, and browser identity cannot select or elevate an
  agent.
- Queen and worker tool discovery is role-scoped. Queen may inspect the roster,
  create and assign tasks, and apply lifecycle transitions. A worker may inspect
  only its current assignment and report Active, Blocked, or Review.
- Workers cannot approve completion, create durable work, message peers, or
  broadcast. Completion remains a Queen/operator decision.
- HTTP and MCP adapters call the same application services. MCP does not call
  HTTP internally and adapters do not duplicate authority rules.
- Claude receives the generated configuration through `--mcp-config`; Swarm does
  not modify a repository's `.mcp.json`.
- The first surface contains task outcomes only. Decision requests and directed
  Queen delivery extend the same principal and service boundary later.

## Consequences

Agent identity survives API restarts without sharing the browser operator token.
Tool descriptions stay small, role-correct, and easy for models to select. A
credential compromise is limited to one worker's authority and can be rotated.

Adding the launch argument advances the terminal-host protocol once. Future MCP
tool additions remain API-only and preserve running PTYs. Local static bearer
credentials are appropriate for the loopback provider-to-daemon path; this does
not decide remote third-party MCP OAuth.

## Alternatives considered

- Port the legacy MCP catalog: rejected because it exposes obsolete machinery,
  broad authority, and duplicated rules.
- Reuse the browser operator token plus a worker query parameter: rejected
  because any worker could forge Queen identity and the shared secret has an
  unnecessarily large blast radius.
- Write `.mcp.json` into every repository: rejected because Swarm should not
  silently mutate source workspaces or create mergeable secret files.
- Hand-write the MCP transport: rejected because protocol negotiation changed
  materially across recent revisions and an official Rust SDK exists.

## Validation

- Persistence migration and restart tests prove credential durability and
  digest-only storage.
- Protocol and provider tests prove `--mcp-config` is retained for new, resume,
  and continue launches.
- MCP contract tests prove unauthenticated calls fail, role-specific discovery,
  worker assignment isolation, Queen-only creation/assignment, and lifecycle
  enforcement.
- A local runtime dogfood session on a non-tunnel port lists and invokes the
  correct scoped tools and survives an API-only restart without replacing the
  Queen PTY. The local Claude process loaded the private configuration; its
  natural-language invocation was blocked by that isolated CLI's login state,
  so an authenticated deployed Queen/worker smoke test remains the final gate.
