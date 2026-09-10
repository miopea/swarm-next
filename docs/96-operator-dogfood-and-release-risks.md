# Operator dogfood checklist and remaining release risks

September 10, 2026. This is a handoff over the approved scope in45, not a new
definition of completion. No release is authorized. Detailed receipts remain
in92,94 and95. Last verified app/API: cb915ab1; live worker engine was preserved.

## What is ready to try

- Needs You: ask a non-authorizing clarification before choosing a final answer;
  Queen or the requesting worker can reply. Quick choices and custom final text
  stay separate. Waiting on a reply is not an operator answer or new approval.
- Queues: follow an operator-owned item to its exact request, or a prerequisite
  to its upstream task. This navigation is verified; automatic backlog clearance
  is not implied.
- Tasks: open history, browse Older activity, return to Latest activity. Only one
  bounded page is retained. Failed reads can retry; late cancelled reads cannot
  replace the current page. Reopening starts at latest.
- System/Settings: mobile navigation closes the expanded System tray; diagnostics
  separates machine capacity from measured pressure. Saved Night Watch schedules
  are distinct from unsaved edits and current presence. Invalid schedule input
  gets correction guidance rather than an unchanged retry suggestion.

## Short operator pass when convenient

Use a demo worker and fictional work for actions that might send input or change
state. Do not test against a real customer approval merely to tick a box.

1. **Ordinary desktop workflow:** use one to five workers, move between terminals,
   Needs You, Queues and task details. Check whether the next owner/action is
   understandable without opening several unrelated pages. Report the exact
   item and wording if it is still unclear; do not manually repair ownership to
   make the test pass.
2. **Clarification on Android:** on a fictional request, ask a question, switch
   away and return, then read the reply before choosing the final answer. Confirm
   that the waiting/replied/answered states make sense. Do not repeat the already
   accepted AskUser questions2/3 or task-editor Keep editing checks unless changed.
3. **Phone history and runtime:** browse older task history, return to latest,
   then open System -> Diagnostics. Check controls are reachable and navigation
   exposes the destination. Desktop rendering is not physical-device acceptance.
4. **Native attachments:** from the installed PWA, test one camera image and one
   gallery image in a demo worker. Confirm visible progress/failure, one successful
   insertion, and that the worker can actually access the shared artifact. This
   remains necessary on Android and iOS; a synthetic upload is not this evidence.
5. **Device handoff:** in the demo terminal, leave an unsent draft, background the
   phone, then use Resume Here on desktop and return to the phone. Check stable
   dimensions, preserved draft and no duplicate input. Record device, time and
   whether the keyboard was visible for any failure.
6. **Presence in normal use:** during the configured Night Watch window, verify
   the active mode, phone use and a genuine desktop return. Distinguish the saved
   schedule from the current mode. Do not infer activation from the enable box
   alone. Testing through desktop automation can itself produce return activity.

These checks do not ask the operator to approve all remaining maturity work.
Native-device results should be attached to the exact build and journey, not
treated as a blanket mobile signoff. No urgent interruption is needed overnight.

## Risks that remain open

| Area | Actual remaining boundary |
| --- | --- |
| Direct answers and duplicate requests | Terminal/AskUser answers still need authenticated capture and exact decision correlation; worker prose alone is not permission. |
| Queen and worker orchestration | Automatic recovery, genuine blocker reconciliation, release handoff and safe protected-input/background-work cases remain. A queue presentation fix does not close the loop. |
| Worker-engine updates | Safe automatic admission, durable receipts and partial-stop/return recovery are unfinished. Do not apply an engine update based only on a Resting label. |
| Conversation recovery | Chosen provider conversation, native continue and clearly identified fresh fallback need remaining real failure/cancellation acceptance. No automatic provider switching. |
| Efficiency and metrics | A matched fresh/aged workload, sustained plateau and attribution remain; test counts and Queen run counts do not prove productivity or efficiency. |
| Provider and native-device coverage | Android/iOS picker, suspension/handoff, real scheduled/manual presence and experimental-provider exclusions need their actual gates. |
| BFG support loop | Fictional attachments and linked task completion/reply approval remain under existing contracts. Admin owns customer sends; no BFG worker contact without approval. |
| Visual approval | Optional While you were away composition remains mockup-gated. Do not introduce an unapproved automatic modal. |
| Integrated acceptance | A current-build ordinary workday/overnight and operator acceptance remain. The full45 goal is not complete. |

## Evidence hygiene

Before any later release, confirm latest CI, exact deployed app/API identity,
worker continuity, and the tested engine version separately. Record a pending
check as pending. Do not count a browser refresh, a green unrelated suite or an
old operator acceptance as proof of a new behavior. Keep failed native checks
and workarounds in the risk report even if ordinary desktop use passes.
