# Legacy delta after the first atlas freeze

Status: **Evidence review in progress**

This review covers the 98 commits added to Legacy Swarm after the atlas froze on
2026-08-10. It is a companion to bounded Swarm Next Ring 1 use, not a backlog
generator. A legacy change is evidence that an operator outcome mattered; it is
not proof that Next has the same defect or should reuse the same mechanism.

## Boundary and confidence

- Range: ledger sequences 1,432 through 1,529, commits `1455bf19` through
  `1f559e84`, dated 2026-08-10 through 2026-08-16.
- Shape: 56 fixes, 15 features, 13 release commits, 5 tests, 5 documentation
  changes, and 4 other changes.
- Most-touched clusters: 90 testing/quality, 39 worker, 37 provider, 31 task,
  30 drone, 27 Queen, 20 settings, 16 terminal, 10 resource, 10 worker-state,
  and 9 Jira commits. Tags overlap.
- Packaged boundary: `3d477fc9` released `2026.8.13`. The 71 commits after it
  remain development evidence. Their tests and incident narratives matter, but
  they have not yet demonstrated stability through a Legacy release.

The unusually high fix-to-feature ratio makes this delta most useful for
discovering hard system boundaries: input authority, truthful delivery,
effect-based safety, state ownership, migration completeness, reconciliation,
and resource hygiene.

## Preliminary comparison with Swarm Next

These dispositions are hypotheses checked against current Next source and tests.
They remain open to live Ring 1 evidence and operator decisions.

| Legacy evidence | Current Next evidence | Preliminary disposition | Why it matters |
| --- | --- | --- | --- |
| `fe4e1eb4` prevented ordinary automation from answering an open provider prompt. The later `de3870ae` through `b338f1c8` chain needed stable prompt identity, explicit answer/dismiss verbs, read-back, truthful refusal, and recovery of the refused message. | Next recognizes a visible Claude choice menu as `AwaitingOperator`, but coordination delivery does not consult that provider activity before `HostRequest::Write`. Its guarded submission verifies that text rendered before sending Enter; it does not prove the terminal was a free-text prompt rather than a selection UI. | **Relevant redesign; high priority.** | A task brief or Queen instruction must never become an answer to a provider question. Keep typed authority and read-back; do not adopt terminal parsing as unquestioned authority. |
| `ec70c136`, followed by `a562a02f`, `f3057e8f`, and `9d568098`, made every PTY write attributable at the holder choke point without recording typed content. | Next has durable actor provenance for task activity and distinct high-level delivery records, but ordinary operator and automation input converge on `HostRequest::Write { session_id, bytes }`. The terminal host cannot currently answer who wrote a specific Enter, Escape, or text-shaped input. | **Relevant redesign; high priority.** | When a worker changes unexpectedly, diagnosis should be a bounded audit lookup rather than inference. The audit must record actor, target, shape, and outcome—not secrets or terminal content. |
| `97148fb5`, `52eea9f5`, `58339e99`, `85491fc4`, `72c66362`, and `78c1ef4` repeatedly corrected wording and state that treated dispatch as outcome. | Next persists queued, delivered, rejected, and uncertain delivery states and uses a render marker before Enter. It therefore prevents much of the false-success class, but an acknowledged final write is still not semantic proof that a prompt was accepted or work began. | **Already partly prevented; retain as an invariant.** | Operator and Queen language must distinguish requested, written, observed, and completed. Never collapse them into “sent” or “worked.” |
| `93284b41` fixed a migration whose schema-version ceiling had not been advanced, so the shipped migration could never run. | Next has one `CURRENT_SCHEMA_VERSION`, forward migration functions, old-version upgrade tests, and integrity verification. The architecture is stronger, but the version ceiling and newest migration are still manually coordinated. | **Mostly prevented; add a mechanical guard.** | A migration that compiles but never runs is worse than a visible startup failure. A test should make the newest step and current version impossible to drift silently. |
| `da7d8a10` found `tempfile.mktemp` leaking one file per test helper call until 16 GB accumulated and disk reached 95%. | Production Next uses owned Rust temporary files/directories in the inspected paths. However, the Outlook test helper currently calls `tempfile::tempdir().unwrap().keep()`, which deliberately leaves that directory behind. | **Confirmed small recurrence; fix locally.** | This is not the 16 GB incident, but it violates the same cleanup invariant and will accumulate under repeated suites. No product decision is required. |
| `1f559e84` added board-versus-Jira divergence to a daily verification sweep; `3bbea838` added a close-time citation check. | Next fetches every already-linked Jira issue, including terminal states, reconciles mapped Jira state transactionally, and maintains durable outbound transitions. Completion also requires concise verification evidence. It does not yet run Legacy's project-specific citation or branch-containment checks. | **Core divergence already prevented; verification policy is optional.** | Jira closure must converge automatically. Project-specific proof rules should be typed, measured policies only where they reduce real review failures. |
| `814876ca` through `26796719` repeatedly repaired approval rules: compound commands, credential reads, outbound payloads, local-versus-remote hosts, permission modes, and deny regressions. | Next deliberately removed the approval-rule regex engine. Deterministic coordination invokes narrow typed application operations; provider-native permission mode remains the provider's authority. | **Obsolete mechanism; keep the safety lesson.** | Do not recreate a second shell-policy language. New deterministic actions need narrow effect contracts and fail-closed tests, not verb matching. |
| `30394629`, `caa69f25`, `2f7dc307`, `cfeb1b27`, `bee7b275`, and `938d63eb` show settings that existed but were inert, inconsistent, or not durable. | Next routes settings through typed APIs and persistence tests, and recent dogfood proved several settings live. | **Already reduced; continue rendered proof.** | A visible setting is an operator promise. Its saved value, runtime effect, and restart behavior must be tested together. |
| `7887087a`, `2167b201`, `30394629`, `37afbf8a`, and `f01a065a` refined the distinction between assigned, active, resting, sleeping, paused, and idle work. | Next separates durable task lifecycle from provider activity and worker lifecycle, with sleeping meaning unloaded. Ring 1 has already exposed the importance of scan-friendly, truthful worker state. | **Outcome kept through redesign.** | Worker state is operational control, not decoration. Task assignment must never imply execution, and quiet must never imply absence. |
| `2f7dc307` added operator-defined worker shortcuts, followed immediately by fixes for an inert list and missing persistence. | Next has mobile terminal controls and configurable worker roster behavior but no equivalent arbitrary shortcut system. | **Optional opportunity.** | Only consider it after real-use repetition identifies commands worth promoting. Avoid adding a general macro surface before a concrete need. |

## Validated chains to expand next

The first atlas validated six chains. This delta justifies five additional
deep checks against implementation and final state:

1. **Provider prompt authority:** trace every automated terminal writer, the
   open-prompt guard, stable prompt identity, answer/dismiss behavior, read-back,
   and refused-message recovery.
2. **PTY write provenance:** verify actor propagation at the single write choke
   point, content-free audit shape, unknown-actor visibility, and retention.
3. **Dispatch truth:** distinguish enqueue, write acknowledgement, rendered
   evidence, provider acceptance, task start, and eventual outcome.
4. **Effect-based safety:** preserve the lesson from the drone approval chain
   while documenting why its regex mechanism is intentionally absent in Next.
5. **Resource and schema hygiene:** verify temporary artifact ownership and
   mechanically link migration ceiling, latest step, startup, and integrity.

## Questions reserved for a real product choice

These are not requests for immediate operator input. They become decision packets
only after the implementation comparison and Ring 1 evidence are complete.

- When Queen sees a provider selection prompt, should she only notify, recommend
  an option, or answer under an explicit confidence/permission policy? Different
  prompt classes may require different authority.
- How much content-free PTY write history is useful to an operator: recent events
  in diagnostics, downloadable private evidence, or internal support data only?
- Which completion checks are truly cross-project invariants, and which belong to
  repository-owned configuration or skills?
- Do repeated real commands justify named worker shortcuts, or do existing mobile
  controls and natural-language messages cover the outcome with less surface area?

## Next evidence step

Check the five chains against exact Legacy diffs and executable tests, then map
each surviving outcome to a current Next owner and test. During the first week of
Ring 1 use, record only observed overlap. The first operator discussion should be
a small packet of genuine choices, not a tour of all 98 commits.
