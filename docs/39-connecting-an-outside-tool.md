# Connecting an outside tool

Swarm already talks MCP. A worker's Claude reaches the board through
`swarm-terminal-host mcp-proxy`, which speaks stdio locally and forwards to
`POST /mcp` on the API with a per-worker bearer token. That path works and is
not what this document is about.

This is about the other direction: a tool that is **not** a worker on this
box — the operator's own Claude client, a laptop, another machine — connecting
to the board over the network.

## What is already true

Measured against `swarm2.bfgsolutions.net` on 2026-08-28:

| Probe | Result | Means |
| --- | --- | --- |
| `POST /mcp`, no credential | `401`, `www-authenticate: Bearer` | reachable, and it fails closed |
| `GET /mcp` | `405` | no server→client stream |
| `/.well-known/oauth-protected-resource` | `404` | no discovery |
| `/.well-known/oauth-authorization-server` | `404` | no discovery |

So the endpoint is **already externally reachable and already authenticated
per-principal** — `agent.rs` resolves the bearer to a worker agent credential,
not to the operator token, so board writes are already attributed to whoever
called. The transport is a single JSON-RPC request per POST.

That is sufficient for `mcp-proxy`, which supplies the header itself and never
needs to discover anything. It is not sufficient for a remote client, and the
gap is not the network — it is that **a remote client has no way to obtain a
credential.**

## The mistake worth not repeating

The task was first read as "the mechanism already exists; the work is a client
principal type and a Connections card." That reading came from `POST /mcp`
returning 401 rather than 404 — a live endpoint behind auth looks finished.

It is wrong, and the legacy repo says why. `swarm-legacy` carries
`tests/test_oauth_mcp.py`, whose own docstring names the thing it guards:

> Regression guard for the Claude Desktop "Connect" flow.

Legacy did not hand a bearer token to a remote client. It ran an **OAuth 2.0
authorization server** — `src/swarm/auth/oauth_server.py`, 309 lines — because
that is the handshake a remote MCP client performs. The connector in the
operator's client is a survivor of that, and it now reads ⚠ Reconnect because
the server behind it is gone.

**A 401 proves a door is locked. It does not prove anyone has been given a key.**

## What legacy actually implemented

From `oauth_server.py` and the routes in `server/api.py`:

| Piece | Detail |
| --- | --- |
| Discovery | `/.well-known/oauth-protected-resource`, `/.well-known/oauth-authorization-server` |
| Challenge | `WWW-Authenticate: Bearer resource_metadata="<base>/.well-known/oauth-protected-resource"` |
| Registration | `POST /oauth/register` — Dynamic Client Registration |
| Redirect guard | allow-list; `https://claude.ai/api/mcp/auth_callback` accepted, `https://evil.com/cb` refused |
| Authorize | `GET /oauth/authorize` — auth code + PKCE, **requires a dashboard session cookie** |
| Consent | a rendered consent page, 10-minute validity |
| Token | `POST /oauth/token` — code exchange and refresh; codes single-use |
| Lifetimes | access 1h, refresh 30d, code 5m |
| Scope | `mcp` |

Two details are load-bearing and easy to lose:

- **The challenge header must carry `resource_metadata`.** swarm-next returns a
  bare `Bearer`. A client that gets that has been told it needs a token and not
  told where to get one, so it cannot start the flow — the 401 is a dead end
  rather than an invitation.
- **`/.well-known/` must answer `404`, not `401`.** `api.py:384` marks the whole
  prefix public with the comment that the MCP SDK probes discovery and must get
  a 404. Putting discovery behind auth makes the probe indistinguishable from a
  server that requires credentials to tell you how to get credentials.

## Decisions taken (interview, 2026-08-28)

| Question | Answer |
| --- | --- |
| **Identity** | Its own client principal — minted and revoked per tool, named in board writes. Not the operator token, not a worker's credential. |
| **Surface** | Same as a worker: file, transition its own work, record deployments, read. It **cannot** approve or assign — those stay Queen's. |
| **Minting** | Settings → Connections. Operator: *"Yes, setting connections. that way it stays in the clients"* — the credential lives in the client, Swarm holds the registration. |
| **Presence** | Separate from the roster. No session, no autostart, and it **must not** count toward resource admission or the live-worker count. |

The minting answer and OAuth agree better than a copy-paste token would: with
Dynamic Client Registration the client registers itself and holds its own
refresh token, and Settings → Connections becomes the place the operator *sees
and revokes* registered clients rather than a place they copy a secret out of.
Nothing sensitive is ever displayed for the operator to move by hand.

The presence answer has a concrete consequence. ADR 0040 admits automatic
worker starts against resource pressure, and ADR 0042 serializes wakes. A
connected client is neither a worker nor a session; counting one would throttle
real workers on behalf of a tool that consumes no PTY and no engine.

## Scope

**In:**

1. OAuth discovery, registration, authorize+PKCE, token and refresh on the API,
   with the redirect allow-list.
2. `WWW-Authenticate` carrying `resource_metadata`; `/.well-known/` public and
   404-on-absent.
3. A client-principal type distinct from a worker agent credential, resolving in
   `agent.rs::authenticate` alongside the existing path.
4. Settings → Connections: list registered clients, last-used, revoke.
5. The principal is refused approve and assign, and is excluded from resource
   admission and the live-worker count.

**Out:** streaming (`GET /mcp`) — every current tool is request/response, and no
measurement here says otherwise. Revisit when a client needs it, not before.

## Acceptance

Each of these fails loudly if the thing it names is absent. None is satisfiable
by reading source.

1. **Discovery answers before auth.** `GET /.well-known/oauth-protected-resource`
   and `/.well-known/oauth-authorization-server` return `200` with no credential;
   an absent well-known returns `404`, never `401`.
2. **The 401 names where to go.** `POST /mcp` with no credential returns `401`
   whose `WWW-Authenticate` contains `resource_metadata=` pointing at a URL that
   itself returns `200`. *Ablation: strip the parameter; the test fails.*
3. **A full flow issues a working token.** register → authorize (with a session)
   → token → `POST /mcp` succeeds; the same call without the token is `401`.
4. **PKCE and single-use are enforced.** A token exchange with a wrong verifier
   is refused, and replaying a consumed code is refused.
5. **The redirect allow-list bites.** Registering `https://claude.ai/api/mcp/auth_callback`
   succeeds; an off-list host is refused. *This is the ablation — a test that only
   registers the good URI proves nothing.*
6. **Authorize is not open.** `GET /oauth/authorize` without a dashboard session
   does not issue a code.
7. **Attribution.** A board write through a client token names that client, and
   is distinguishable from a worker's and from the operator's.
8. **The ceiling holds.** The client principal is refused approve and assign.
9. **It is not a worker.** With a client connected, resource admission and the
   live-worker count are unchanged. *Ablation: count it; the test fails.*
10. **Revocation is immediate.** Revoking in Settings → Connections makes the
    next `POST /mcp` with that token `401`.
11. **Seen, not just tested.** The Connections surface is looked at through the
    harness (`docs/38`) at desktop and narrow widths before it is called done.

## Not proven here

The operator's existing connector points at the legacy server. Nothing in this
document has been run against a real client — the flow is reconstructed from
legacy's tests and from probes against the live endpoint. **A green test suite
is not evidence that Claude's Connect button works.** That needs the operator to
add the connector and reach the board, and until that happens this ships as
implemented, not verified.
