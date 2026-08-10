# M0 evidence review

Status: **Draft recommendation for operator review**

This review narrows the replacement to outcomes that are both central to daily
operation and necessary to test the new architecture. It is not a promise to
reproduce every legacy feature.

## Evidence standard

Evidence is weighted in this order:

1. Direct operator experience and observed failure modes.
2. Current provider capabilities and documented interfaces.
3. Legacy routes, schemas, tests, and recent code evolution.
4. Privacy-safe production telemetry, if an aggregate export is later made
   available.

The inspected WSL database is an isolated development instance: it contains 11
workers and 10 buzz records, but no tasks, messages, pipelines, playbooks, or
Queen activity. It is useful for confirming schema breadth, not for inferring
production usage. No task text, messages, secrets, terminal output, or config
values were read.

## What the legacy system says

The legacy application has accumulated several product-shaped subsystems. A
May-August 2026 structural sample shows where implementation effort has been
concentrated:

| Area | Files | Approximate lines | Commits touching area |
|---|---:|---:|---:|
| Web | 28 | 24,901 | 195 |
| Server | 63 | 18,511 | 169 |
| Drones | 30 | 8,435 | 63 |
| MCP | 40 | 7,462 | 50 |
| Database | 16 | 4,343 | 35 |
| Tasks | 12 | 3,621 | 51 |
| PTY | 9 | 3,325 | 14 |
| Queen | 10 | 2,606 | 15 |
| Pipelines and playbooks | 12 | 1,500 | 13 |

Commit count and line count are complexity signals, not usage telemetry. They
show that rebuilding the entire surface before proving the terminal and worker
model would recreate the highest-risk coupling first.

The legacy approval subsystem spans hooks, provider-screen interpretation,
rules, tuning, state tracking, and decision execution. Current Claude tooling
already exposes provider-owned permission modes, tool allow/deny lists, and a
permission-prompt integration point. The legacy source itself notes that the
native sandbox handles routine approval traffic. Swarm should therefore stop
owning routine provider tool-prompt approval.

Current interface references:

- [Claude Code CLI permission controls](https://docs.anthropic.com/en/docs/claude-code/cli-usage)
- [Codex approval and sandbox configuration schema](https://github.com/openai/codex/blob/main/codex-rs/core/config.schema.json)

## Recommended product cut

### Walking skeleton

- Persistent provider terminal sessions owned outside the browser and API
  process.
- Explicit worker lifecycle, health, exit, restart, and recovery.
- Fast worker switching without reconnecting or redrawing other sessions.
- Minimal task loop: create, assign, activate, review, complete.
- Direct input with an explicit writer lease and stale-session rejection.
- Sequence-based terminal replay with bounded memory and durable checkpoints.
- One provider adapter, local single-operator authentication, diagnostics,
  backup, update compatibility checks, and rollback proof.

### First core expansion

- Second provider adapter.
- Small agent coordination protocol for findings, blockers, handoffs, task
  state, and operator decisions.
- Search and activity/audit views.
- Browser notifications and privacy-safe feedback bundles.
- Resource policy for host pressure and controlled worker recovery.
- GitHub integration behind a typed, least-privilege boundary.

### Deferred until a measured journey requires them

- Queen assignment and conversational coordination.
- Groups and bulk actions.
- Jira, Outlook, remote tunnel, and mobile/PWA packaging.
- Context-pressure intervention beyond provider-native behavior.
- Pipelines, playbooks, standing loops, speculative preparation, cross-project
  tasks, and broad legacy history import.

### Do not port as subsystems

- Routine approval drones.
- Provider-prompt regex approval and self-tuning rules.
- Provider-specific screen scraping where a declared adapter event or hook is
  available.
- Separate crash, pressure, and proposal subsystems when those outcomes belong
  to worker lifecycle, resource policy, or the operator decision inbox.

## End-user result

The first visible improvement is not a new dashboard. It is a workspace that
behaves like durable equipment:

- Reloading the browser returns to the exact terminal position.
- Switching workers is immediate and cannot disturb another session.
- Updating or restarting the API does not kill active work.
- Slow or disconnected clients cannot grow browser or server memory without a
  bound.
- Worker state is explicit rather than inferred from CSS, terminal text, or
  timers.
- Failures identify the responsible layer and offer a recovery action.

## Recommendation

Approve this cut as the M0 product baseline. Begin implementation only after
the provider-first choice and the terminal retention policy in
`09-open-questions.md` are resolved. Other deferred capabilities can be tested
through dogfooding without blocking the walking skeleton.
