# ADR-0001: Architecture-first replacement

Status: **Accepted**

## Context

Legacy Swarm is a heavily used, rapidly evolved application whose implementation
contains valuable behavior alongside obsolete automation, compatibility paths,
and overlapping state ownership. A mechanical language port would preserve the
same liabilities and obscure which features remain valuable.

## Decision

Develop Swarm as a separate repository and application. Legacy Swarm
remains operational and serves as behavioral evidence. Each capability is
classified as keep, redesign, merge, remove, or investigate before it becomes a
Swarm requirement.

Runtime scaffolding follows approval of the M0 definition set.

## Consequences

- Legacy production stability is insulated from replacement work.
- Feature parity is not a release goal by itself.
- Some existing features will intentionally not return.
- Product and architecture review is a formal development milestone.
- Side-by-side operation and explicit migration are required.

## Alternatives considered

- In-place rewrite: rejected because it couples daily-driver stability to the
  replacement and encourages structural translation.
- Long-lived rewrite branch: rejected because it complicates releases, review,
  and eventual history while still encouraging legacy coupling.
- Incremental cleanup only: rejected as the sole strategy because the current
  boundaries limit the desired persistent-session model.

## Validation

M0 produces an approved capability inventory, journeys, architecture,
walking-skeleton contract, and dogfooding plan before runtime implementation.

