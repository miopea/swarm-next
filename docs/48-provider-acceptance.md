# Provider acceptance and promotion

This is the PROV-01 acceptance record template, not a provider promotion.
Availability, successful launch, and a passing adapter test do not establish
maturity. Only the builder authorizes promotion. Workers never switch providers
automatically. Experimental providers require explicit opt-in and are excluded
from new Night Watch automation.

For each provider record its CLI version, Swarm revision, engine protocol, host OS,
browser/PWA versions, test date, evidence links and remaining limitations. Record
each row as passed, failed, unverified, or unsupported. Unsupported required
capabilities prevent promotion; missing evidence is not a pass.

| Required journey | Evidence needed |
| --- | --- |
| Install and authenticate | Clean supported installation, unavailable executable, expired credentials, and recovery without exposing secrets. |
| Interactive terminal | Ordinary output, output bursts, narrow/wide resize, cursor movement and multi-question prompts remain readable. |
| Operator input | Enter, arrows, paste, multiline composition and interrupted input preserve exactly the intended submission. |
| Attachments | Camera and gallery on Android/iOS, file references, failed upload and retry reach the selected conversation without duplicate submission. |
| Swarm tools | Current scoped tool discovery, task reads, valid outcomes and authorization rejection work after launch and rolling updates. |
| Permission and questions | Provider questions remain distinguishable from idle prompts; automation does not answer or overwrite them accidentally. |
| Conversation continuity | Explicit selection becomes the default; sleep/wake and graceful stop preserve it; missing-context fallback uses native continuation before the authorized fresh/manual path. |
| Failure recovery | Crash, API interruption and unavailable provider preserve durable task ownership and report uncertain delivery instead of replaying it. |
| Task lifecycle | Initial brief, active work, blocked reason, review question/answer and evidence-backed settlement agree across Queen, worker and Queues. |
| Provider change | An explicit operator handoff transfers context/task details; no automatic fallback silently changes provider. |
| Updates | Quiet rolling restart converges, including tool freshness and a worker that is normally always active; conversation evidence remains correct. |
| Unattended work | Engagement guards, safe kicks, pressure holds/recovery and Night Watch exclusion/promotion are verified end to end. |

Run focused adapter and state-transition tests for affected code. Reserve real
provider/device exercises for the journeys they can establish; do not require an
unrelated full platform matrix for every ordinary patch. New provider promotion,
however, requires all applicable journeys, with failure and recovery evidence.

## Current implementation boundary — 2026-09-09

Rechecked against main `b1e993d4` and the live App/API and engine
`1.6.0-dev-cf83c980bf17-20260909041327-2251164`. The historical implementation
notes below are superseded by this checkpoint; this is not a provider promotion.

- Host-owned discovery reports Claude and Codex available; Gemini, Grok and
  OpenCode are explicitly unavailable. No alpha CLI was installed for this check.
- Creation, changed bindings and temporary-worker commands enforce explicit
  acknowledgement plus positive host evidence. Unknown/absent capability refuses
  the change. Unrelated edits preserve an existing experimental binding.
- Live Edge Settings initially offered Claude/Codex. Enabling the unsaved opt-in
  revealed all three alpha choices labelled unavailable and the explicit warning
  about missing Swarm tools, automatic recovery and Night Watch eligibility.
  Opt-in was cleared and the test tab closed without saving or creating a worker.
- The current source gates supervisor startup, coordinator startup, Queen startup,
  retained-worker revival and the common coordination submission path on provider
  policy. Policy lookup failure refuses automation; it does not switch provider.
- The builder-owned promotion list remains Claude/Codex. No UI setting changes it.
- Focused web verification passed 43 tests across WorkerSettings (22), temporary
  experimental handoff (3), worker API requests (2) and held-briefing presentation
  (16). This includes failed-save retention, withdrawn consent, unknown discovery,
  unchanged bindings and duplicate-submit prevention. These tests do not launch a
  real alpha provider or prove its terminal behavior.

- Current Linux verification also passed both domain admission tests, the builder
  promotion policy test, the old/new engine availability fixture, the persistence
  Night Watch hold/return test, and five API route/delivery tests. The hold/return
  test covers all three alpha providers; the shared submission test covers both
  immediate and cooled delivery without contacting the terminal.

Remaining acceptance: demonstrate an installed alpha's launch/manual-recovery
limitations only in a disposable fixture when that provider is available. The provider-specific
promotion journeys above remain separate from acceptance of the opt-in framework.
Do not interpret their unverified rows as permission to enable Night Watch.

### Historical implementation sequence (September 5)

`ProviderKind::NIGHT_WATCH_APPROVED` currently contains Claude Code and Codex.
This reports the existing builder-owned policy, not a claim that this maturity
program has completed every acceptance row for either provider.

Gemini, Grok and OpenCode are excluded by automatic wake/briefing eligibility and
the shared last pre-submission coordination gate. Regression tests verify that
Night Watch holds preserve work and attempts, and ending the watch restores
eligibility. These are deterministic policy tests, not real-provider acceptance.

The worker-creation UI currently offers only Claude Code and Codex, and the API
availability view exposes only those two. A coherent explicit experimental opt-in
and availability contract remains unfinished. Existing experimental workers must
still show their real provider in settings and preserve it when unrelated details
are edited. Showing an existing binding does not promote it or add it to the
new-worker picker.

ADR 0079 defines the pending admission contract: host-owned optional availability,
explicit acknowledgement when creating/changing an experimental binding, honest
bare-adapter limitations, and unchanged builder-owned Night Watch admission.
This design is not fully implemented and is not a provider promotion. Older-engine omission
must remain unknown, and unrelated edits must retain the existing provider.

The first implementation step adds optional, host-owned executable availability
for the three existing alpha adapters and a domain rule requiring both explicit
acknowledgement and positive availability for a new experimental binding.
Two domain tests and one old/new IPC compatibility test passed in the isolated
Linux export; API and terminal-host compile checks passed there with the changed
files overlaid. This is not a full clean-current-tree test run. Command enforcement,
operator UI, temporary-worker acknowledgement and end-to-end acceptance remain
pending. The foundation is not deployed and enables no experimental provider.

The next local step enforces acknowledgement on HTTP worker creation, changed
provider bindings and temporary siblings, before mutation. Provider updates hold
the lifecycle lock across the current-binding check and save. Five focused API
tests passed in the isolated Linux export, including actual routes against
old/absent/positive host capability evidence and preservation of the parent.
The first test fixture used PUT rather than PATCH; that fixture was corrected
before the passing run. The TypeScript client carries acknowledgement only when
explicitly supplied; both client tests and TypeScript checking passed. Settings
opt-in, temporary-worker confirmation, failed-save retention and browser testing
remain pending. This backend step is not deployed by itself.

Builder sign-off must identify the evidence revision and any supported-platform
limits before changing the promotion list. Do not turn elapsed soak time or
operator availability into automatic promotion.
