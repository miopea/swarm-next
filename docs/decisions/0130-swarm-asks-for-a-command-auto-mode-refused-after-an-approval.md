# 0130 — Swarm asks for a command auto mode refused after an approval

- Status: Accepted
- Date: 2026-10-29
- Decided by: the operator, "build it" (2026-10-29, about 02:10Z), after the
  read-only review of how ADR 0129's grant was used (note 25824 on 01a182c8).
  Task 01a18576.
- Amends: ADR 0129, an approved command runs once in the session that asked.
  Narrows ADR 0123's "never a refused tool's input" for the grant mod only.

## Context

ADR 0129 lets a worker run a command once when the operator presses Swarm's
grant button on a request for that exact command. In its first six hours on
the operator's Hive (2026-10-28 19:50Z to 2026-10-29 02:05Z, note 25824 on
task 01a182c8), no worker filed one. 18 of the 22 decisions in that time were
raised by Queen. Three auto-mode refusals happened. Two of them came after the
operator pressed an approval that Queen had raised on the refused worker's own
task:

| Worker | Refused command | What happened next |
| --- | --- | --- |
| Real Truth | `gh pr merge 171` | Refused at 22:10Z, after approval 01a1847a ("Merge PR #171, no deploy"). The operator typed it into the session with `!` at 22:14Z. |
| Platform | A production read | Refused at 00:27Z and dropped. Note 25824 names no approval before it, so this change may not have caught it. |
| Public Website | `az webapp deployment slot swap` | Refused at 02:00Z, after approval 01a1850a ("Swap production to ced6f0d"). Approved through `/permissions` at 02:05Z. |

A grant belongs to the worker and session that asked for it, so Queen's
approvals could not become one. The two refused workers left the command to
the operator, through `!` or `/permissions`, instead of asking with `command`.
The grant only works if the worker asks, and these workers did not.

Probed on Claude Code 2.1.312 in auto mode (2026-10-28, a scratchpad mod and a
stand-in script that only echoes):

- A mod's `classic.PermissionDenied` hook fires on a classifier refusal. Its
  input carries `tool_name`, the exact `tool_input.command` that `tool.check`
  saw, `tool_use_id`, the classifier's `reason` (for example
  `[Production Deploy]`), `cwd` and `permission_mode`.
- It fires inside `tool.call`'s `next(e)`. The refusal comes back to
  `tool.call` as `{ isError: true, text: "Permission for this action was denied
  by the Claude Code auto mode classifier…" }`. Returning
  `{ deny: text + note }` after that delivered the note to the model word for
  word, and the command did not run.
- Claude drops a leading `cd <dir> &&` before every hook when the shell is
  already in `<dir>`, so the refused text can be relative to the shell's
  folder. `e.cwd` and `$.session.cwd()` agreed.

## Decision

- **The grant mod reports auto-mode refusals.** Its `classic.PermissionDenied`
  hook awaits `next(e)` and leaves Claude's result unchanged. For a Bash
  command refused in auto mode, it runs `swarm-terminal-host provider-refused`
  with the command, the shell's folder and Claude's reason. That helper posts
  to `/mcp/command-refused` with the worker's own credential.
- **Swarm files the request itself, and only after an approval.** It looks
  for the worker's task in its running session: an unreleased assignment, not
  completed, abandoned or removed, Active first. It then looks for the latest
  approval on that task that the operator resolved by pressing an action. A
  dismissal, an answer in words, an input decision and a command request's
  own grant do not count. With no task or no such approval, Swarm files
  nothing and says nothing.
- **The request is Swarm's, word for word.** Its only button is Swarm's grant.
  Its title, summary, reason, risk and evidence are written by Swarm, and name
  Claude's reason and the approval it followed. The worker writes none of it.
  The card marks it as filed by Swarm from a field no worker can set
  (`command_refusal`), so a worker's own request cannot borrow that line.
- **The command is bound to its folder.** Swarm files `cd <folder> &&
  <command>`, quoted as ADR 0129's matching expects. A relative command such as
  `./release` cannot then be granted in one folder and run in another. When
  the worker runs it again from that folder, Claude checks it without the
  `cd`, and ADR 0129's matching recognises it. A command that already starts
  with a `cd` into an absolute folder, written exactly as one of those forms,
  runs the same from anywhere. It is filed as Claude checked it, not behind a
  second `cd`. Anything less certain is bound.
- **It does not pile up.** A refusal of the same command while its request
  waits points at that request. A worker has at most three such requests
  waiting (`MAX_PENDING_REFUSED_COMMANDS`). Past that, Swarm files nothing and
  says so. A worker's own command requests are not counted.
- **The worker reads what to do beside the refusal.** The mod's `tool.call`
  hook adds Swarm's note after Claude's refusal text. The note names the
  decision and the approval, says not to ask the operator to type it, run it
  another way or change permissions, and to carry on or move the task to
  Blocked on that decision. When the operator answers, ADR 0129's resolution
  line tells the worker whether it was granted. Swarm does not move the task
  itself.
- **Everything else is ADR 0129 unchanged.** The grant is minted only by the
  press, is exact, once, and bound to the worker, the session and the task.
  Dismissing tells the worker no grant was made.

## Threat model

| Threat | Mitigation |
| --- | --- |
| A worker fabricates a refusal to get a card | It gains what a command request already gives it: one card for one exact command, granted only if the operator presses it. The card shows the command in full. |
| A worker floods the inbox through refusals | One card per command while it waits, and at most three waiting per worker. |
| A worker words a card to look like Swarm's | Swarm's line on the card comes from `command_refusal`, which only this path sets. |
| A relative command is granted in the wrong folder | It is filed bound to the folder it was refused in. |
| The operator's approval is read as covering the command | The card quotes the approval and asks about the command separately. Approving the work never grants a command. |
| Swarm is down or slow | The helper waits at most 5 seconds, prints nothing on any failure, and the refusal reads as it would have. |

Residual risks, accepted:

- **The worker can reach its own credential.** Its shell can read its MCP
  config and post a refusal that did not happen. That is the threat in the
  first row, and it buys nothing a command request does not.
- **The latest approval is a heuristic.** The approval may be about something
  else on the task. The card names it so the operator can judge.
- **The refused input now reaches Swarm.** ADR 0123 forwards no refused input
  from the turn hooks. This path sends the command text, which is what the
  operator must read before granting it. It goes only to the worker's own Hive
  and is stored on the card, as a command request's text already is.

## Transition

- The API half (the route, the store, the card) applies on a reload.
- The engine half (the mod's two hooks, `provider-refused`) applies with the
  worker engine update. Until then no refusal is reported.
- Schema 214 adds `refusal_reason` and `refusal_follows_decision_id` to
  `decision_requests`.

## Evidence

### End to end, 2026-10-29

Run on the isolated Hive from ADR 0129's run, rebuilt from this branch. It had
its own socket, port, database, agents folder and workspace, and one throwaway
Claude 2.1.312 worker in auto mode holding one Active task. The command was
`cd <folder>/fp && ./release-to-production --app demo-shop --slot production --skip-review && echo released`,
run from the workspace folder. The script is a stand-in that only echoes, so
nothing was deployed. Times are UTC. "Refused" means the auto-mode classifier
refused it (Production Deploy). Each prompt asking the worker to attempt the
command said no production change was approved in that message.

| Case | Result |
| --- | --- |
| Refused with nothing approved on the task | 02:54:59. Swarm logged the report and asked nothing (no approval). The worker read Claude's refusal unchanged. No card. |
| The operator approves the work | 02:56:52. The worker was told the answer granted no command, and waited. |
| Refused after that approval | 02:57:58. Swarm filed a card: only Swarm's grant button, Swarm's wording naming the approval and Claude's reason, `command_refusal` set. The worker read Swarm's note under Claude's refusal, then looked the decision up itself. |
| The same refusal again | 02:59:35. The same card, not a new one. The worker read "Swarm already asked". |
| The operator presses the grant | The worker was told "Granted" with the exact command. Told to run it, it ran once in the same session at 03:02:04, and the grant was spent. |
| A different command (one flag added) | Refused at 03:03:16. No grant spent. Swarm filed a separate card for it. |
| The operator dismisses that card | 03:04:09. The worker read "No grant was made" and did not run, retry or ask again. |
| The same command, after the absolute-`cd` change below | Refused at 03:07:44 and filed exactly as Claude checked it. Granted, it ran once at 03:09:16, and the grant was spent. |

The run found one fault, fixed and tested here. The command already started
with a `cd` into an absolute folder, and the first build bound it to the
shell's folder too, so the card read `cd <workspace> && cd <workspace>/fp && …`.
It granted and ran correctly, but it was harder to read. Such a command is now
filed as written.

Not shown end to end:

- The bound of three waiting cards. It rests on its unit and route tests and
  their ablations.
- A worker running the granted command unprompted. Each prompt in the run told
  the worker not to retry, and it waited to be told. The note tells it to run
  exactly the granted command once the operator allows it.
- A real fleet worker. That needs the reload and the worker engine update.

### Ablations, 2026-10-29

Each row removes one mechanism and runs the test that guards it. Every test
passed on the unchanged tree first. In every row below the test failed with
the mechanism removed. Each file was restored byte for byte afterwards, and the
branch's diff was identical before and after the run.

| Acceptance | Mechanism removed | Failing test |
| --- | --- | --- |
| 1 | The worker reads Swarm's note in the route's answer | `a_refusal_after_an_approval_puts_the_grant_button_up_through_the_route` |
| 1 | The helper passes on only a note with words in it | `a_refusal_is_reported_and_only_swarms_note_comes_back` |
| 1 | The card's Swarm line reads only `command_refusal` | `a command Swarm filed after a refusal says Swarm asked, not the worker` |
| 1 | Claude's reason loses its control characters | `a_refused_command_is_granted_only_in_the_folder_it_was_refused_in` |
| 2 | Only an approval counts (an answered input counted) | `a_refusal_is_asked_about_only_after_an_approval_on_the_workers_task` |
| 2 | A command request's own grant is not an approval | `a_refusal_is_asked_about_only_after_an_approval_on_the_workers_task` |
| 2 | A dismissal or an answer in words approves nothing | `a_refusal_is_asked_about_only_after_an_approval_on_the_workers_task` |
| 3 | One card per command while it waits | `a_refused_command_is_asked_about_once_and_few_wait_per_worker` |
| 3 | At most three waiting per worker | `a_refused_command_is_asked_about_once_and_few_wait_per_worker` |
| 3 | A worker's own command requests are not counted | `a_refused_command_is_asked_about_once_and_few_wait_per_worker` |
| 6 | The command is bound to the folder it was refused in | `a_refused_command_is_granted_only_in_the_folder_it_was_refused_in` |
| — | A command that already changes into an absolute folder is filed as written | `a_refused_command_is_granted_only_in_the_folder_it_was_refused_in` |

The six store rows (acceptance 2 and 3) were run again after the approval
lookup moved into its own function, and each failed again.

One more row found nothing to guard. Skipping the first form when matching an
absolute `cd` changed no result, because that form is the text after the `cd`
and can never equal the whole command. The skip was deleted.

The mod's two new hooks have no automated test. Their evidence is the run
above: with nothing approved the refusal read unchanged, and after an approval
the worker read Swarm's note under Claude's text. They are validated with
`claude plugin validate` and type-checked against the 2.1.312 types, and the
type check failed when a field name was broken.

## Consequences

- An approved piece of work whose command auto mode refuses needs one more
  press, on a card Swarm puts up, rather than a command typed by hand.
- A worker that is refused with nothing approved is still left to ask with
  `command` itself.
