# Using Swarm

Swarm runs a group of coding agents — **workers** — on one machine, and gives
you one place to see what they are doing and one queue for the things only you
can decide.

The idea it is built around: an agent that needs you should ask **in a queue you
choose to read**, not by interrupting whatever terminal you happen to be looking
at.

## The five screens

The rail on the left is the whole product.

**Needs you** — the one queue. Everything waiting on your judgment: a worker's
question, an email nobody has answered, a Queen review that stopped. Routine
work never appears here. When it is empty, nothing needs you, and that is the
answer rather than an absence of one.

**Tasks** — the board. Work that survives sessions and restarts. A task carries
its own description, its state, and who owns it. Tasks come from you, from Jira,
from an imported email, or from a worker that found follow-up work.

**Workers** — the roster and the terminals. Every worker is a real provider
process in a real repository. Selecting one shows its live terminal; you can
type into it exactly as you would in a shell.

**Apiary** — other Hives, when you federate. Ignore it if you run one machine.

**Settings** — crew, presence, Queen, alerts, system, integrations, backup,
diagnostics.

## Workers

A worker is a name, a provider, and a repository. It is not a chat window: it is
a persistent session that keeps its context between your visits, and Swarm keeps
its terminal alive whether or not you are looking at it.

If a repository moves on disk, edit the worker in Settings → Crew and change
its path. The worker has to be asleep, and moving it forgets its saved
conversation — the provider files history under the project path, so the old
thread belongs to the old repository.

**Queen** is the one worker you do not create. She coordinates: she notices work
that needs routing, assigns it, and asks you when she cannot decide. Everything
she does is bounded — she can be set to coordinate only, and external effects
stay blocked unless a rule you approved covers them exactly.

Worker state is a colour and a word:

| State | Meaning |
| --- | --- |
| **Buzzing** | Working right now. |
| **Resting** | Loaded, idle, waiting for something to do. |
| **Sleeping** | Not running. Costs nothing. Wakes when work arrives. |
| **Awaiting you** | It asked a question and stopped. It is in Needs you. |
| **With you** | You are driving it from this screen or another device. |
| **Blocked** | Something is wrong that it cannot get past. |

## How work reaches a worker

You do not paste tasks into terminals.

1. You write a task, or one arrives from Jira or email.
2. You assign it — or Queen does.
3. Swarm writes the briefing into that worker's terminal itself, waits until it
   can see the text has rendered, and only then sends Enter.

That last part sounds fussy and is not. A provider redrawing its input can
swallow a keypress, so Swarm watches the screen rather than trusting a delay. If
it cannot confirm the message landed it says so on the worker rather than
sending it twice — **a briefing delivered twice is worse than one you were told
about**.

## When a worker needs you

A worker that needs a decision files it and stops. It appears in **Needs you**
with what is being decided in the first line, and the reasoning folded behind
it.

You can answer three ways: press one of the offered buttons; say something none
of them offered, in your own words; or decline with a reason. Declining needs a
reason, so "hold this for now" and "stop asking me" cannot be recorded as the
same thing.

Your answer is delivered back to the worker that asked, and it carries on
without needing to be woken.

## Presence

Swarm tracks whether you are **at the Hive**, **away**, or on **night watch**,
and Queen's autonomy follows it. It is automatic by default — activity, window
visibility, screen lock — and you can override it in Settings → Presence.

The point is not surveillance. It is that an agent should behave differently at
two in the morning than when you are sitting in front of it.

## Two screens on one worker

Open the same worker on a laptop and a phone and both show the live terminal.
The device **holding** the worker decides its size; the other watches. "Work
here" takes it, without sending the worker anything — reclaiming a screen and
instructing an agent are different acts.

## Email and Jira

**Jira** — bind a project in Settings → Integrations and issues become tasks,
with comments flowing both ways.

**Email** — a message becomes a task. The worker does the work, records where it
is running, and drafts the reply. **You review the words and send it.** Swarm
never sends mail on your behalf without you reading it, and a ticket merged from
several messages by one person is answered once, on the thread they wrote in
last.

## Keeping it running

The runtime area at the bottom of the rail shows the version and anything
waiting. A worker engine update and a provider restart are always offered; an
App and API rebuild appears only if this Hive was pointed at a working copy.

Swarm asks you once whether to check for new releases, and contacts nothing
until you answer. A check sends no version, no identity and no counts — it
fetches one signed file and compares it here.

When a release is offered, downloading and installing are separate acts, because
one is reversible and the other is not. Installing runs on its own and **the
page reloads itself** when the new version is healthy; your workers keep running
throughout.

Most releases change only the app, and then the worker engine simply moves with
it and nothing restarts — Swarm decides that by fingerprinting the engine's own
source rather than the release number. A release that genuinely changes the
engine restarts loaded workers, so it waits until they are idle and tells you
before anything happens. `docs/install.md` covers all of it, including
installing a release by hand.

Every update asks first, and the warning is proportional: an App and API release
keeps your workers online and says so, while a worker engine or provider restart
names exactly what stops before offering the button.

**Settings → Diagnostics** answers one question: which layer needs attention. It
leads with a verdict, shows only what is not normal, and puts everything else
behind a count. It also states the machine — memory, processors, whether it is
under pressure — because six gigabytes of workers means something different on
thirty-two than on eight.

## Things worth knowing early

- **Sleeping workers are normal.** A sleeping worker costs nothing and wakes
  when work arrives. A roster of resting workers is a healthy Hive, not an idle
  one.
- **Completion is not deployment.** A task can be finished without being proven
  live. Swarm labels that "finished · unverified" rather than pretending.
- **Nothing is retried silently.** When Swarm cannot confirm something landed it
  tells you and stops, rather than trying again behind your back.
- **The database is one file.** `~/.local/state/swarm/swarm.sqlite3`. Back it up
  by copying it. Settings → Backup does this properly, including verifying that
  the copy opens.
