# Issue sources, and where feedback goes

Status: **Being scoped 2026-08-22.** Nothing built. Decisions below are the
operator's; the reasoning and the open parts are recorded so this is not
re-litigated from memory.

## What prompted it

Two facts, one measured and one remembered.

Dogfood feedback filed in Swarm lands in `dogfood_reports`, a table on the Hive
it was filed from. There is a `GET /api/v1/feedback/reports` and nothing else:
no path off the machine. **Zero reports exist on the operator's Hive** — they
say it out loud to the worker instead, which their developers cannot do.

And when Swarm Legacy was being shut down, three GitHub issues from one
developer, four months old, surfaced for the first time.

A developer with an idea currently has nowhere to put it that anyone will read.

## Decided

**GitHub Issues becomes a source, like Jira.** Bind the repository, issues
become tasks, comments flow both ways. The reason is reach: pushing feedback
into GitHub means every Swarm user can file, not only members of one Apiary.

**Credentials are per developer, by OAuth**, as Jira and email already are. A
developer running Swarm pulled it from GitHub and already has an account, so
this costs them nothing and removes anonymous filing — which is most of the spam
answer.

**Issues arrive as drafts, not ready work.** Queen passes over them to make them
ready, or to spot duplicates that want merging. This costs the operator nothing,
keeps the task board from becoming a public inbox they triage, and pushes the
volume problem far enough out that it may never arrive.

**The bottleneck concern is a success problem.** Five issues a day is a blip.
Legacy managed three in four months. If volume ever makes the operator the
bottleneck, the product is popular and the input flow can change then.

**Duplicate detection is deferred** for the same reason. Queen merging on the
draft pass covers the near term.

**Not per-source implementations.** Where this is going is one framework with
several sources — Jira, GitHub, email, local, Apiary — rather than a third
parallel copy of the same code.

## What is already right, and should not be redone

The `tasks` table has **no source-specific columns**. Not a Jira key, not a
message id. Sources attach through link tables — `jira_issue_links`,
`email_message_links` — so a task is a task and the source hangs off it.

GitHub is therefore additive rather than invasive: another link table beside the
others. The separation of concerns exists at the data layer already.

## The consolidation with an actual payoff

Not "abstract the source". Intake genuinely differs per source; forcing one
shape on all of them is how an abstraction earns its bad name.

What is duplicated is **outbound delivery**: `jira_comment_deliveries`,
`jira_transition_deliveries`, `email_reply_deliveries` are three tables doing
one job — a durable queue with retry and delivery state. Every new source adds
two more. That is identical across sources and worth unifying, and it is where
the replicated code lives.

## Open, and worth settling before building

**The state-mapping question that is already filed and unanswered.** "An
unmapped Jira state freezes the LOCAL lifecycle too — decide whether local state
should advance when the Jira write is held." GitHub's model is open and closed,
nothing like a Jira workflow, so a shared mapping would be fitting two unlike
things into one shape while the underlying rule is undecided. Answer it first.

**How much a closed issue says.** Corrected during this discussion: Swarm
already transitions the linked Jira issue on every task state change, so a
completed task does close its ticket. What it does not do is say anything — no
comment explains what changed, the way the email path composes a reply someone
actually reads.

For internal Jira that is probably fine; the reporter is a colleague who can
look. For a public GitHub issue from a stranger, a silent close reads as being
ignored, which is the same discoverability failure from the reporter's side. The
machinery is shared, the bar may not be. Decide per source rather than by
default.

**Whether GitHub is templated on Jira or on email.** Structurally it resembles
Jira — issues, comments, states. In the way that matters it resembles email:
there is a person waiting for an answer, and a reply is public. Email already
requires the operator to review before sending; a Jira comment does not. Copying
Jira posts public comments on the project's own repository with less care than
Swarm applies to email.

## Charm, and its one rule

Held to the same standard as everything else: **it must cost no cognition.**

The operator's example is the right kind. Every worker draws the same bee, so
thirty-one workers look like one worker thirty-one times. `BeeMascot` already
takes a role and an expression and draws inline SVG, so a per-repository
variation is a third axis on the existing two — a stable hue and marking derived
from the workspace path. Nothing to draw, nothing to configure, no meaning to
learn. You simply start recognising a worker the way you recognise a colleague.

It must never be the only carrier of meaning: state stays a word and a colour.

The failure mode is the opposite kind — whimsy that has to be decoded.
"Adjusting terminal layout…" was cute about something that was not layout, and
it cost the operator time. The bee does the charm; the sentence stays honest.

## Parked

The drone surface, and the question of measuring how much the Hive does without
the operator. Both revisited at v1.0. The argument for them is in
`docs/33-keeping-the-hive-moving.md`; the reason for parking is the operator's:
a log that does not produce something actionable is not worth being visible, and
the actionable version is this feedback loop.
