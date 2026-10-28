# 0129 — An approved command runs once, in the session that asked

- Status: Accepted
- Date: 2026-10-28
- Decided by: the operator, in a directive on task 01a182c8 (2026-10-27):
  "a restart or resume per approval is not acceptable. An approval must take
  effect in the CURRENT session, with no /permissions edit. Use a Claude Code
  mod's tool.check, not the startup settings file." Asked where the mod lives
  and what the script hash covers, they chose "Ship it in Swarm" and "The local
  file it runs".
- Amends: ADR 0003, provider-owned permissions. This is the mediation that
  ADR's revisit condition asks a new ADR and threat model for.

## Context

A worker whose command was denied by a permission check or by auto mode's
classifier could ask for it with `swarm_request_decision` and a `command`.
Swarm then added a grant button. Approvals almost never let the command run.

Measured read-only on the operator's Hive on 2026-10-28, over every decision
that carried a command (29, filed from 2026-09-08 to 2026-10-28):

| How it was resolved | Count |
| --- | --- |
| A button the worker wrote ("Approve", "Run it") | 25 |
| Swarm's grant button | 1 |
| Answered as an interview | 1 |
| Withdrawn | 2 |

Three causes:

1. **The worker's own buttons were required.** The request had to offer at
   least one worker-written action, and the operator reasonably pressed the
   one that read like yes. Those buttons are text the worker wrote, so they
   grant nothing. An interview answer grants nothing either.
2. **The one grant could not have worked.** It was written into the worker's
   settings file as a `Bash(<command>)` rule. Claude reads that file only when
   a session starts, so the grant waited for a restart. It then allowed the
   command for the whole session, not once. Claude also reads a `*` in a rule
   as a wildcard, so a grant for `rm build/*` allowed every command starting
   `rm build/`. That grant was never used.
3. **The worker was not told.** The resolution line said which button was
   pressed ("Action: Merge PR 689, no deploy"). It did not name the command or
   say whether it was granted, so the worker retried into the classifier or
   asked again with another look-alike.

Probed on Claude Code 2.1.312 in auto mode (`claude -p --permission-mode auto`,
2026-10-27), with a compound command running a stand-in release script that
only echoes:

| Setup | Result |
| --- | --- |
| No mod | Denied by the classifier (Production Deploy) |
| Mod allows that exact text in `tool.check` | Ran; the classifier was never called |
| Mod allows a different text, else `next(e)` | Denied by the classifier |
| Mod awaits `next(e)`, allows only a plain `ask` | Ran; the classifier was never called |

The last row shows Claude's own verdict for that command is a plain `ask`
(no rule, hook, setting or plugin named), and auto mode only asks the
classifier after `tool.check` returns.

## Decision

- **A command request offers only Swarm's grant.** It names its task and the
  exact command. Worker-written actions, operator actions and questions are
  refused with a message saying what to send instead. The recommendation is
  empty or the grant itself. The command is at most 4,000 bytes, not blank,
  and holds no control or invisible characters other than newlines and tabs,
  so the card shows what would run.
- **A request may name the script the command runs.** The worker gives its
  absolute path. Swarm hashes the file when the request is filed and the card
  shows the path and sha256. A command that runs no local file matches on its
  text alone.
- **A grant is exact, once, and in the session that asked.** It is minted only
  when the operator presses the grant button, and only if its task is still
  on the board and the session that filed the request is still running.
  Otherwise the resolution says no grant was made. It is live while unspent,
  while its task is on the board, and while that session runs. It is spent
  with a guarded update, so two uses cannot both succeed. If it names a
  script, the file is hashed again at use, and a changed or unreadable file
  is refused without spending the grant.
- **The comparison is equality, plus one rewrite Claude makes.** Before its
  permission check, Claude drops a leading `cd <dir> &&` when the shell is
  already in `<dir>`. A grant for `cd /srv/app && ./release` would then never
  match once the worker's shell had moved into `/srv/app`. So the hook also
  sends the shell's directory, and Swarm compares the checked text, and that
  text behind `cd <the shell's directory> &&`, against the grant by equality.
  The directory is quoted the three ways a worker would write it (bare when it
  needs no quoting, in single quotes, in double quotes) and never matched as a
  pattern. A `cd` into any other folder is a different command.
- **The worker engine loads a Swarm-owned grant mod into every Claude worker.**
  It is written beside the worker's MCP config at each launch and loaded with
  `--plugin-dir`. Its `tool.check` hook for Bash awaits `next(e)` first. Only a
  plain `ask` can become an allow, and only when Swarm spends a grant that
  matches the checked text as above. Every other verdict is returned
  unchanged. The hook asks
  through `swarm-terminal-host provider-grant`, which posts to
  `/mcp/command-grant` with the worker's own credential. Every failure reads as
  "not granted".

  Awaiting `next(e)` first refines the directive's "otherwise returns
  `next(e)`". A deny rule or a hook's decision still wins over a grant, and a
  command the worker could already run does not spend one.
- **The settings-rule grant is removed.** It was start-only, session-long, and
  wildcarded on `*`. A stand-down no longer needs to spend grants, because a
  grant from an ended session is never live.
- **The resolution says what happened to the command**, naming it exactly, or
  as "the command in this request" when it spans lines:
  - When granted, the worker may run that text once. Any other text is not
    covered. If it is still denied, it moves the task to Blocked with the
    denial quoted, rather than retrying or asking again.
  - When not granted, it does not run or retry it. If the operator's note says
    to go ahead, it asks once more with the same command.
  - An approval with no command says it granted none, and points at `command`.
- **The tool says when to use `command`.** Its description tells a worker
  denied by a permission check or the classifier to ask with `command`, since
  no worker-written button grants anything.

## Threat model

| Threat | Mitigation |
| --- | --- |
| A worker mints its own grant | Only the operator's resolution with the exact grant label mints one. A command request cannot carry worker buttons. |
| A worker spends another worker's grant | The route authenticates the worker's own credential, and a grant belongs to one worker. |
| A grant covers more than its text | Equality in SQL. No pattern or wildcard reading exists. The only other form is the same text behind a `cd` into the folder the shell is already in, which runs the same thing. |
| A grant is replayed | Single use, by a guarded update in one statement. |
| A grant outlives its purpose | Dead with its task, or when the session that asked ends, however it ends. |
| The script is swapped after approval | Hashed at filing and again at use. A changed file is refused. |
| The card hides what runs | Control and invisible characters are refused. The card shows the whole text, the script and its hash. |
| Swarm is down or slow | The hook answers "not granted" after at most 5 seconds of waiting on Swarm, and the worker's own checks decide. |

Residual risks, accepted:

- **Hash to run.** A process that can write the script can still change it in
  the milliseconds between the hash at use and the run. A script can also run
  other files that are not hashed.
- **The credential, not the session, is what Swarm sees.** The route knows the
  worker by its bearer token, not by which Claude process asked. Any process
  holding that token can spend the worker's live grant while the asking
  session runs: a subagent, a second shell of the same worker, or anything
  that reads its MCP config. It can spend only that exact command, once.
- **The mod is not a sandbox.** It runs as the operator's user. A worker able
  to rewrite its own mod folder could equally rewrite its Claude settings.
  This adds no route around the worker's checks that editing settings does not
  already give.
- **Cost.** Every Bash command that Claude would ask about now also starts the
  helper and makes one local request before the classifier runs.

## Transition

- The API half (validation, grants, the route, the resolution lines, the card)
  applies on a reload.
- The engine half (the mod, `--plugin-dir`, `provider-grant`) applies with the
  worker engine update. The terminal protocol is unchanged.
- Between the two, a grant lets nothing run. The settings rule it replaces is
  gone, and the old engine has no mod. The resolution line then tells a worker
  whose granted command is still denied to move the task to Blocked.
- Only Claude workers load the mod. A grant to another provider's worker lets
  nothing run, and that worker is told to go Blocked in the same way.

## Evidence

### End to end, 2026-10-28

Run on an isolated Hive built from this branch. It had its own socket, port,
database, agents folder and workspace, and one throwaway Claude 2.1.312 worker
in auto mode. The command was compound:
`cd <folder> && ./release-to-production --app demo-shop --slot production --skip-review && echo released`.
The script is a stand-in that only echoes, so nothing was deployed. Times are
UTC. "Denied" means the auto-mode classifier refused it (Production Deploy).

| Case | Result |
| --- | --- |
| The worker asks after a denial | It filed `swarm_request_decision` with the command, the script and the right sha256, unprompted. The card offered only Swarm's grant. |
| The operator presses the grant | The resolution named the exact command. The worker ran it in the same session at 15:19:45, with no restart and no settings edit. Swarm logged the grant spent. |
| A grant from an earlier session | The second session's attempt was denied at 15:51:25. The grant was not spent. |
| The worker's shell is already in the script's folder | Claude checked the text without the `cd`. It ran at 15:56:09 on the `cd` form, and the grant was spent. |
| The task left the board before use | Denied at 15:58:57. The grant was not spent. |
| A live grant, and a different command (one flag added) | Denied at 16:04:46. The grant was not spent. The granted command then ran at 16:06:16. |
| The same command again, after it ran | Denied at 16:07:40. |
| The operator dismisses the request | The worker read "No grant was made: … do not run it or retry it". It did not run, retry or ask again. No grant row was written. |

None of the worker's settings files gained a rule during the run.

Not shown end to end: refusing a grant whose script changed. Its one live
attempt (15:38:10) predates the `cd` fix below. Claude checked the text
without the `cd`, so that build could not have matched it whatever the hash,
and the denial does not tell the two causes apart. The hash check rests on its
unit test and its ablation.

The run found two faults, both fixed and tested here before the rows above
were taken:

- A grant pressed after the coordinator had completed its task was minted
  dead, while the worker was told "Granted". Minting now requires the task on
  the board.
- Claude's `cd` rewrite (in Decision above) made a correctly granted command
  hit the classifier.

One case did not behave as the brief expected. A different command is not
always denied: once, a variant of the granted command (with
`&& echo released again` appended) was allowed by the classifier on its own,
with no grant spent. A grant never covers a different command. Such a command
goes through the worker's normal checks, and those checks may allow it. The
card says exactly that.

### Ablations, 2026-10-28

Each row removes one mechanism and runs the test that guards it. Every test
passed on the unchanged tree first. In every row the test failed with the
mechanism removed, the file was restored byte for byte afterwards, and the
branch's diff was identical before and after the run.

| Acceptance | Mechanism removed | Failing test |
| --- | --- | --- |
| 1 | Refusing worker-written actions on a command request | `a_command_request_cannot_offer_an_approval_that_grants_nothing` |
| 1 | Minting only on the grant button (any resolution minted one) | `a_command_grant_needs_the_operator_to_press_the_button_that_names_it` |
| 2 | "No grant was made" in the resolution line | `a_resolution_says_whether_a_command_was_granted` |
| 3 | Loading the grant mod with `--plugin-dir` | `a_grant_mod_folder_reaches_the_launched_command` |
| 3 | Recognising the `cd` Claude drops | `a_grant_matches_the_command_claude_checks_after_dropping_a_cd_into_its_folder` |
| 4 | Equality (a grant covered any command starting with its text) | `a_command_grant_needs_the_operator_to_press_the_button_that_names_it` |
| 4 | Single use (a spent grant stayed live) | `a_command_grant_needs_the_operator_to_press_the_button_that_names_it` |
| 4 | The grant dies with its task | `a_grant_ends_with_its_task_or_its_session_and_belongs_to_one_worker` |
| 4 | The grant dies with its session (it passed to the next) | `a_grant_ends_with_its_task_or_its_session_and_belongs_to_one_worker` |
| 4 | No minting for a task already off the board | `a_grant_ends_with_its_task_or_its_session_and_belongs_to_one_worker` |
| 4 | No minting after the asking session ended | `a_grant_ends_with_its_task_or_its_session_and_belongs_to_one_worker` |
| — | The script hash recheck at use | `a_grant_runs_only_while_its_script_still_hashes_as_approved` |
| — | Opening the script without blocking (a FIFO held the API thread) | `a_script_that_is_not_a_regular_file_is_refused_without_waiting` |

Rows sharing a test failed at different assertions, one for each mechanism.

The mod's own verdict logic has no automated test. Its ablation is the probe
above: with no mod the command was denied, and with the mod it ran.

## Consequences

- Approving a command is one press of one button, and it takes effect in the
  running session.
- Decisions that ask for a command and something else must be split into two.
- Grants made before this change are never live again, because none recorded
  a session.
