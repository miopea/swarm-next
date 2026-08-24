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

## Settled during scoping

**A held Jira write no longer freezes the task.** The local lifecycle advances,
the Jira write is queued, and a missing mapping fails at delivery where it is
visible — rather than aborting the local transition with it. Recorded as
[ADR 0052](decisions/0052-a-held-jira-write-does-not-freeze-the-local-lifecycle.md),
which is where the ruling and its cost now live.

The filed question priced this third option as "a queue and a reconciliation
path that does not exist today". It does exist: `jira_transition_deliveries`
already queues transitions and delivers them asynchronously. What is wrong is
only *where the mapping is resolved* — `queue_jira_transition` looks it up at
queue time, inside the same transaction as the local move, so an absent mapping
takes the local transition down with it. Moving the lookup to delivery time is
most of the work.

That answers the filed question with its own evidence: one misconfigured Jira
project stopped thirteen tasks moving inside Swarm, and no external
configuration error should be able to do that.

**A closed issue says something, on every source.** The state change alone
leaves the person who reported it with nothing to read.

**But the register follows the audience, not the machinery.** GitHub issues come
from developers, so a technical answer is right. Jira and email reach staff and
the public, who need to know their issue was handled without being told how.
Half of this is already specified — the email draft tool instructs the worker to
write "what changed, what they can do now, and no internal implementation
detail" — so GitHub becomes the exception that permits technical language, not
the other way round.

**A GitHub comment is reviewed before it posts.** It is public, permanent, and
attributed to the operator's own account on the project's own repository. Email
already works this way and the habit exists; Jira posting directly is
defensible because the audience is internal, and public comment is not the same
act.

## Open, and worth settling before building

**Whether the state mapping itself is shared across sources.** GitHub is open
and closed; Jira is a configurable workflow. Now that a held write no longer
freezes anything, the pressure to unify them is lower — a shared mapping table
would be fitting two unlike models into one shape for its own sake. Worth
deciding deliberately rather than by momentum.

**Where the reviewed GitHub comment is reviewed.** Email replies appear in Needs
you with the words in front of the operator. A GitHub comment could reuse that
exactly, or sit on the task. Reusing it means one queue and one habit, which is
the product's whole thesis, but it puts more into Needs you.

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
