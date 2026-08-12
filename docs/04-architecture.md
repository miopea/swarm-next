# Target architecture

Status: **Proposed**

## Shape

Swarm Next is a modular Rust monolith with a React/TypeScript browser client and
an embedded SQLite database.

```text
React application
  |-- generated HTTP client
  |-- application event stream
  |-- replaceable presentation components
  `-- framework-independent terminal controllers

Rust application
  |-- API and authentication adapters
  |-- application services
  |-- domain modules
  |-- integration adapters
  |-- persistence boundary -> SQLite
  `-- local IPC -> independent Rust terminal host -> PTYs and workers
```

The terminal host is a dedicated process in the same product package. This
process boundary preserves PTY descriptors across API replacement; it is a
deployment boundary, not a second product backend.

## Proposed modules

- `identity`: authentication, sessions, authorization, attach grants.
- `apiary`: optional Hive federation, membership, scoped stewardship, project
  catalog, atomic claims, and cross-Hive routing.
- `workspace`: roots, provider binding, runtime configuration.
- `tasks`: task lifecycle, dependencies, assignment, history.
- `workers`: durable worker identity and worker-session lifecycle.
- `terminal`: PTY ownership, canonical terminal state, synchronization.
- `orchestration`: recommendations, policies, bounded guarded dispatch, verification.
- `decisions`: unified operator attention, resolution, and guarded durable delivery.
- `messages`: findings, handoffs, blockers, operator communication.
- `integrations`: external adapters and synchronization boundaries.
- `mcp`: agent-facing tools over application services.
- `persistence`: migrations, repositories, transaction boundary, backup.
- `observability`: traces, metrics, diagnostics, feedback bundles.
- `platform`: service management, update, rollback, resource policy.

## Architectural constitution

1. Domain modules do not import HTTP, WebSocket, UI, or database drivers.
2. HTTP and MCP adapters invoke the same application services.
3. Only persistence repositories issue application SQL.
4. An integration submits commands; it cannot update core state directly.
5. One transaction commits a domain transition and its durable activity event.
6. Terminal bytes do not travel through the general application event bus.
7. Terminal session lifetime is independent of API and browser lifetime.
8. Every process, task, connection, subscription, and background loop has an
   owner and explicit shutdown behavior.
9. Every buffer, queue, retry loop, history, and concurrency pool is bounded.
10. Correctness cannot depend on sleeps, delayed retries, or repeated layout
    events. Timing can optimize behavior, never establish truth.
11. Session-scoped commands carry immutable session identity.
12. Public contracts are versioned and validated at every trust boundary.
13. Compatibility code documents its owner and deletion condition.
14. Unsafe Rust is isolated, documented, and reviewed at a module boundary.
15. A subsystem reports health using evidence it owns.
16. UI component lifetime never owns worker, terminal, connection, replay, or
    canonical-dimension lifetime.

## Data ownership

SQLite is the source of truth for durable application state. The database is
not shared with legacy Swarm. Import reads a consistent copy or export.

Recommended rules:

- WAL mode with measured checkpoint policy.
- Short explicit transactions.
- One migration system and forward-only production migrations.
- Transactional backup and restore verification.
- Foreign keys and domain invariants enforced at both schema and service level.
- Durable event/outbox records committed with the state they describe.
- Terminal output is not stored in general activity tables; bounded session
  persistence is designed separately.

## Contract strategy

- Rust types define HTTP schemas and OpenAPI.
- TypeScript clients and models are generated in CI.
- Application events use a versioned envelope with monotonic resume cursor.
- Terminal synchronization uses its own binary-capable protocol and sequence.
- Contract fixtures test backward compatibility during rolling updates.

## Security boundaries

- Local-only and remote-access modes have distinct threat models.
- Browser terminal access uses short-lived, worker-session-scoped grants.
- Secrets never appear in query strings, activity events, or diagnostic bundles.
- Worker processes receive a minimized environment.
- Filesystem roots and executable policies are explicit per workspace.
- Integration credentials are accessible only to their adapter.
- The terminal supervisor validates local peer identity and command limits.
- Audit records distinguish operator, automation, integration, and agent actors.
- A Hive is the execution and credential boundary. Apiary authorization can
  coordinate a Hive but cannot bypass its filesystem, provider, or terminal
  boundaries.
- Steward authority is an explicit durable scope grant, never inferred from
  organizational labels or model output.

## Explicit non-goals for the foundation

- Microservices.
- A general plugin runtime.
- Distributed databases.
- Shipping multi-user collaboration in the first local dogfood slice. The
  identity and ownership model is nevertheless Apiary-ready per ADR 0010.
- Mechanical compatibility with every legacy endpoint.
- Reimplementation of provider-native automatic approval.
