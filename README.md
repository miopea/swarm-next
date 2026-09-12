# Swarm

Swarm is a persistent control room for a group of AI coding agents. It runs on
one Linux machine as your own systemd user services, keeps agent sessions alive
across browser reloads and application updates, and gives you a single queue for
the decisions only you can make.

It installs under your home directory, runs as you, and listens only on
localhost. There is no server to administer, no container to orchestrate, and no
account to sign in to.

![The Swarm control room: the rail on the left with Needs you, Tasks, Workers and Settings, and the one queue open, showing a worker asking the operator whether a slow export should fail or write a partial file.](docs/images/needs-you.png)

Every worker keeps a terminal that survives browser reloads and application
updates, so you can read what one is doing and take the keyboard at any point.

![A worker's terminal in Swarm: the rail and roster on the left with four workers, and Orchard Web selected, showing its session working through a redirect loop — reading the route and guard, naming the two rules that conflict, and reporting which tests passed and what it did not verify.](docs/images/workers.png)

## Install it

```sh
curl -fsSL https://raw.githubusercontent.com/miopea/swarm-next/main/install.sh | sh
```

If you would rather read a script before running it — a reasonable habit, and
this one is short:

```sh
curl -fsSLO https://raw.githubusercontent.com/miopea/swarm-next/main/install.sh
less install.sh
sh install.sh
```

There is no version to look up. The script reads the same signed manifest every
running Hive reads to decide whether an update exists, so a fresh install and an
upgrade can never disagree about what "current" means, and it refuses anything
whose digest does not match what the manifest publishes.

It asks you to choose a sign-in token, installs the services, and **waits for
the API to answer before saying it worked**. Then open
**http://127.0.0.1:8766**.

The same command upgrades an existing Hive: `swarm-package` decides whether this
is a first install or an update.

[docs/install.md](docs/install.md) has the rest — what it puts on your machine,
what to do on WSL2, installing an exact version by hand, and what to do when it
does not work.

## What it does on its own, and what waits for you

Worth knowing before you install it, because two of these happen unasked.

**It checks for releases about every four hours**, and shortly after it starts.
A check fetches one small signed file and compares it locally; **nothing is
sent** — not your version, not your Hive's identity, not how many workers you
run. Checking is on unless you turn it off in Settings → Updates, and turning it
off means this Hive contacts nothing at all.

**Installing a release always waits for you.** Downloading and installing are
separate buttons, because downloading is reversible and installing is not. Your
workers keep running through an ordinary update: Swarm restarts the API and
leaves the terminal engine they are attached to alone.

**It replaces the terminal engine on its own** when a release changes it, once
no worker reports being mid-turn. That restarts loaded workers. A resting
terminal is not proof that work has finished — a worker waiting on its provider,
or holding input you have not sent, reads as resting — so Swarm records which
workers it unloaded, returns them to their saved conversations, and says on the
**App and API** card afterwards what it did and that nobody asked for it. An
interrupted command is not resumed, and unsent input can be lost.

**A release that changes the terminal protocol cannot preserve running
terminals** — the API and the engine have to move together. That one stops every
worker, and it is the install worth choosing a moment for.

## Upgrading, and your database

Swarm keeps its state in SQLite, and releases carry schema migrations. **Copy
`~/.local/state/swarm/swarm.sqlite3` before a tarball install**: the automatic
backup covers a rebuild from a checkout and does not cover an upgrade from a
downloaded release.

`swarm-package rollback` returns to the previous release. If an update leaves
the Hive unhealthy, [docs/install.md](docs/install.md#if-an-update-goes-wrong)
is the recovery path.

## Where to go next

| If you want to | Read |
| --- | --- |
| Install it, or fix an install | [docs/install.md](docs/install.md) |
| Use it day to day | [docs/using-swarm.md](docs/using-swarm.md) |
| Move over from the earlier Python Swarm | [docs/moving-from-legacy.md](docs/moving-from-legacy.md) |
| Work on Swarm itself | [docs/developing.md](docs/developing.md) |
| Read the accepted design decisions | [docs/README.md](docs/README.md) |

## Your Claude profile comes with you

Claude runs with your ordinary `~/.claude` profile, so workers inherit the same
credentials, slash commands, skills, hooks, plugins, and conversation history you
already use at the terminal. Swarm points every Claude worker at
`~/.claude/settings.json` explicitly, so permissions and auto-mode policy stay
consistent even when a worker is started by the service rather than a login
shell.

There is no separate profile authentication step: if `claude` works in your
terminal, it works for a worker.

## What it is built on

A Rust modular monolith for the application and the terminal/session backend, a
TypeScript browser adapter rendered with React, and SQLite as the embedded source
of truth behind one persistence boundary. React does not own terminal or worker
lifetime and remains replaceable behind the browser adapter boundary.

Swarm is a ground-up redesign of the earlier Python Swarm — now called Swarm
Legacy — keeping the outcomes that proved useful and replacing accidental
architecture. [docs/developing.md](docs/developing.md) covers building it,
running it from a checkout, and how capabilities are classified before they are
carried over.
