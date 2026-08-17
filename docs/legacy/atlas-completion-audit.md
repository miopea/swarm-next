# Legacy atlas completion audit

Status: **Core archaeology proven; Ring 1 window and operator decisions remain open**

This audit checks the active atlas objective against current artifacts and live
evidence. “Complete” means the named evidence proves the requirement at its full
scope. It does not mean no future Legacy or Ring 1 observation can extend the
atlas.

## Requirement ledger

| Objective requirement | Authoritative evidence | Verdict | Remaining proof |
| --- | --- | --- | --- |
| Review every reachable Legacy commit from root through latest | Generated `commit-capability-ledger.csv` contains 1,529 ordered identities from root `c4aeedd1` through `origin/main` `1f559e84`; the ledger self-test passes, and a direct remote-head check on 2026-08-17 returned the same identity. | **Proven through the latest reachable 2026-08-16 tip.** | Refresh if Legacy advances before Ring 1 closes. |
| Distinguish stable features from experiments and release churn | `stable-release-boundaries.md` samples explicit packages and checks surviving owners; `final-contract-audit.md` compares final claims with implementation and tests. | **Proven for material capabilities.** | Extend only if a new finding depends on an unsampled boundary. |
| Trace regressions, reversions, and operational gotchas | `validated-regression-chains.md` verifies high-value chains against commits and touched owners; `reversions-and-abandoned-experiments.md` preserves failed diagnoses, explicit reverts, and discarded mechanisms. | **Proven for selected material chains.** | Add a chain only when new dogfood exposes a related outcome or missing recovery path. |
| Compare material findings with current Swarm Next architecture and live behavior | `post-2026-08-10-delta.md`, `final-contract-audit.md`, the capability table in `../26-legacy-evolution-atlas.md`, and `../24-browser-dogfood-acceptance.md` name current owners, tests, live evidence, and remaining proof. | **Proven for reviewed material findings.** | Continue the bounded live evidence window; do not infer unobserved equivalence. |
| Classify findings without assuming a port | The atlas uses already-prevented, relevant-redesign, optional-opportunity, obsolete-constraint, and unresolved-evidence dispositions. `drone-disposition.md` demonstrates the mechanism/outcome split for the most misleading high-volume category. | **Proven.** | Preserve the same classification discipline for new evidence. |
| Exercise one bounded week of Swarm Next Ring 1 use in parallel | `ring1-observation-log.md` defines the bounds and formal elapsed window; live evidence already includes provider-question refusal, typed PTY provenance, wide-terminal geometry failure and correction, worker-description generation, Jira image detail, and same-host resource attribution. | **Incomplete.** | A full bounded week has not yet elapsed. Record naturally occurring overlap, failures, recovery, and quiet periods through the declared end of the window. |
| Bring only genuine product choices to the operator | `ring1-legacy-decision-packet.md` removes mechanical fixes and records four evidence-backed operator choices against current Next boundaries. | **Proven and recorded 2026-08-17.** | Reopen a choice only if bounded Ring 1 evidence meets its stated change condition. |
| Close the live wide-terminal outcome | `c27ecc5` has focused and full automated coverage for the observed `7 x 50` transitional fit. The protocol-9 migration revived Queen and live browser proof measured 1,052 by 738 CSS pixels at desktop 1,440 by 900, 398 by 576 at Android-size 412 by 915, and exactly 1,052 by 738 after returning to desktop, with zero overflow or browser errors. | **Proven in code and live.** | Continue ordinary Ring 1 observation; reopen only if a real session reproduces stale geometry. |

## Current completion boundary

The historical archaeology is not waiting on more broad commit reading. The
remaining work is deliberately narrow:

1. refresh the ledger only if `origin/main` moves;
2. finish the bounded Ring 1 observation window without enabling real Queen,
   Jira, email, or Apiary effects merely to manufacture evidence;
3. interview the operator about additional evidence-backed opportunities without
   treating any Legacy feature as an automatic port; and
4. run this audit again against the then-current tree, live release, and evidence
   before declaring the atlas objective complete.

Until all four are satisfied, the atlas remains active.
