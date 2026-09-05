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

## Current implementation boundary — 2026-09-05

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

Builder sign-off must identify the evidence revision and any supported-platform
limits before changing the promotion list. Do not turn elapsed soak time or
operator availability into automatic promotion.
