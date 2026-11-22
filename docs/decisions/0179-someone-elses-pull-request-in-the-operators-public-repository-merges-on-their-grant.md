# 0179 — Someone else's pull request in the operator's public repository merges on their grant

- Status: Accepted
- Date: 2026-11-22
- Decided by: the operator. They said: "I don't want random people working on
  swarm or merging or PRs without my approval. They can submit a PR or they
  can submit a merge request, but I have to approve them." Asked which
  repositories that covers, they answered: "My own repos, and only teh public
  ones, private ones I already gave someone access to" (2026-11-22). Queen
  chose where it is built: in Swarm's grant mod, with the operator's ADR 0129
  grant as their decision on that exact pull request and head.
- Amends: ADR 0131. Queen's instruction no longer covers merging a pull
  request that the merging account did not open, in that account's own public
  repository. Also ADR 0158: a card Queen grants does not let that merge run.

## Context

Swarm's own repository is public, so anyone can open a pull request against
it. GitHub now holds every outside contributor's workflow runs until the
operator approves them. That holds the checks, not the merge.

The merge is made by a worker, signed in to `gh` as the operator. While
"Queen releases reviewed work" is on (ADR 0131), the worker's auto mode
accepts Queen's instruction to merge reviewed work once its checks pass. That
instruction did not ask who opened the pull request. Queen's review was the
only judgment between a stranger's pull request and `main`, and every Hive
installs from `main`.

The fleet's own PreToolUse hook checks a pull request's checks before a
merge. It never checks the author, and it reads only `gh pr merge`.

## Decision

- **The rule.** A merge is covered when all three hold:
  - the repository is public;
  - its owner is the account the merge would be made as, which is the account
    `gh api user` names in the folder the merge runs in;
  - the pull request was opened by any other account. A bot (`is_bot`, an
    `app/` login, or a `[bot]` suffix) never counts as that account, even one
    signed in under the same name.

  Every other merge is unchanged: a private repository, one the account does
  not own, and the account's own pull request.
- **A covered merge runs only on the operator's own grant.** It needs an
  ADR 0129 grant the operator pressed for that exact command. A grant Queen
  pressed (ADR 0158) does not count. The grant mod asks with
  `operator_only`, and the API spends only a grant whose decision has no
  `granted_by_queen`. Like every grant, it runs once, in the session that
  asked, while its task is open.
- **The command names the head.** A covered merge must name its head:
  `--match-head-commit` for `gh pr merge`, or the `sha` field for the merge
  API. The name must be at least 7 characters and a prefix of the pull
  request's current head. Otherwise it is refused before any grant is asked
  for, so the operator's decision covers that head and nothing pushed after
  it. GitHub refuses the merge itself if the head then moves.
- **Where it runs.** The grant mod's `tool.check` hook sees each Bash command
  after Claude's own checks. A command that mentions a merge and runs `gh` or
  `curl` goes to `swarm-terminal-host provider-merge` when it was allowed, or
  would have gone to auto mode's decider. The helper answers in one of three
  ways:
  - `pass`: the worker's own checks decide, exactly as before;
  - `granted`: the hook allows it;
  - `refuse`: the hook denies it with a reason saying how to ask.

  A deny stands as it was. An ask from a rule, hook, setting or other plugin
  is a person deciding, so it is left alone.
- **What it reads.** The helper reads the command as the shell would:
  - quotes, escapes, comments and redirections;
  - `;`, `&`, `|`, newlines and parentheses;
  - `cd` before the merge;
  - `bash -c`, `sh -c`, `eval`, backticks and `$(...)`, up to four levels;
  - `env`, `sudo`, `nohup` and similar wrappers.

  It finds these merges:
  - `gh pr merge`, apart from `--disable-auto`;
  - `gh api` PUT to `repos/<owner>/<repo>/pulls/<n>/merge`;
  - GraphQL `mergePullRequest`, `enablePullRequestAutoMerge` and
    `enqueuePullRequest`;
  - the same endpoint or mutation sent with `curl`.

  For each merge it asks `gh` who the account is, who opened the pull request,
  its current head, and the repository's owner and visibility. Each call has
  a 6-second limit and 256 KiB of output. The hook allows the whole helper 30
  seconds.
- **Every doubt refuses.** A merge it cannot show to be outside the rule is
  treated as inside it:
  - a pull request `gh` cannot read, a folder it cannot tell, a GraphQL or
    `curl` merge whose repository it cannot read, or a merge through `xargs`
    counts as covered, so it must name its head and needs the grant;
  - a command that mentions a merge but cannot be parsed is refused, and so
    is one nested more than four levels deep;
  - a helper that fails, times out or prints anything else, and a hook that
    throws, refuse the merge.
- **The briefs say so.** Queen's release paragraph and the worker's both
  exclude this merge from what her instruction covers. Queen's grant
  paragraph says the card is always the operator's, so she hands it on.

## What this does not cover

The rule is not airtight. This decision covers only merges that pass
through Swarm's grant mod. The fleet's own hook now covers Claude Code
sessions on the operator's machine, as the next point says.

- **The fleet's PreToolUse hook now checks the author, on the operator's
  machine.** The repository that owns that hook added the same rule at
  commit `ad7f4b35`. It refuses a merge only when all three hold: the
  repository is public, the `gh` account owns it, and another account opened
  the pull request. It also reads `gh api` PUT to the merge endpoint, and it
  refuses a GraphQL merge it cannot check. It was installed on the
  operator's machine on 2026-11-22, by the operator's decision.
- **What neither check reaches.** Each of these can still merge a stranger's
  pull request:
  - a Claude Code session on another machine;
  - a Codex worker;
  - any other agent or tool, outside Claude Code's hooks, that holds the
    operator's GitHub credential.
- **What the text does not show:**
  - a script file;
  - a shell alias or function;
  - a GraphQL query read from a file (`-F query=@file`);
  - a merge made by another program.
- **Merging without a merge command.** Fetching a pull request's branch and
  pushing it to `main` (`gh pr checkout`, then `git push origin HEAD:main`)
  merges a stranger's code without any merge this can see, and nothing in
  this ADR stops that push.
- **The repository's own GitHub settings** decide which outside workflow runs
  wait for approval. This ADR does not change them.

## Transition

- **The API half applies on a reload:**
  - `operator_only` on `/mcp/command-grant`;
  - the briefs for sessions that connect after the reload;
  - Queen's review prompt from her next review.
- **The rest applies with the worker engine update:**
  - the grant mod's merge check;
  - `provider-merge`.

  A worker gets the new mod at its next start.
  - **This Hive:** a reload leaves the engine update waiting, and the
    operator applies it from Settings ("Apply worker engine update"). That
    restarts loaded workers, so it is the operator's step.
  - **Member Hives:** apply it by themselves when their workers rest
    (ADR 0156).
- **Until both halves run, nothing changes.** An older API ignores
  `operator_only` and would spend a grant Queen pressed too. A release carries
  both halves, and this Hive reloads before its engine update.
- No schema change and no terminal protocol change.

## Evidence

- **Unit tests in the helper**:
  - every shape of merge is found, with its folder, repository and head;
  - reads and other commands are not merges;
  - only someone else's pull request in the account's own public repository
    is covered, bots included;
  - a covered merge must name its current head;
  - a merge that cannot be read counts as covered;
  - with a stub `gh` and a stub grant endpoint, a stranger's pull request is
    refused, then merges once the operator grants it, and the request asks
    for the operator's own grant only;
  - anything that cannot be checked is refused.
- **Storage and route tests:** a grant Queen pressed is not found by the
  operator-only lookup, and is not spent by the route. The operator's own
  grant is spent once. Both still serve an ordinary grant check.
- **Ablations, each caught by a failing test:**
  - the author ignored;
  - visibility ignored;
  - owner ignored;
  - bots trusted;
  - the head not required;
  - a moved head allowed;
  - an unreadable merge passed;
  - the grant ignored;
  - newlines not splitting commands;
  - `eval` not read;
  - the Queen filter removed in storage;
  - `operator_only` ignored by the route.
- **Probed on Claude Code 2.1.330** in auto mode, with an allow rule for
  `gh pr merge`, a stub `gh` and a stub grant endpoint:

  | Probe | Setup | Result |
  | --- | --- | --- |
  | Old mod | a stranger's pull request | merged |
  | New mod | no grant | refused with this ADR's text; the worker asked once and did not merge |
  | New mod | the operator's grant | merged |
  | New mod | the account's own pull request | merged, no grant asked |
  | New mod | the helper missing | refused |

- **Not proven:**
  - no real GitHub merge was made;
  - no real Hive has run the new engine;
  - the hook's ordering against other plugins' `tool.check` hooks was seen
    only with the plugins on this machine.

## Consequences

- Merging a stranger's pull request into the operator's own public repository
  takes one press from the operator, at a head they saw. Queen's review still
  comes first.
- A worker merging the operator's own pull request, or any private or other
  owner's repository, sees nothing new. The only cost is a parse of a command
  that mentions merging.
- A refusal costs a worker one decision request. A wrong pass would merge
  outside code into the repository every Hive installs from.
