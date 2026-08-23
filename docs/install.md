# Installing Swarm

Swarm runs as a set of systemd **user** services on one Linux machine. It is not
a server you administer and not a container you orchestrate: it installs under
your home directory, runs as you, and listens only on localhost.

## Install it

Three commands, nothing to click:

```
curl -fsSLO https://github.com/miopea/swarm-next/releases/download/v0.7.0/swarm-0.7.0-linux-x86_64.tar.gz
tar -xzf swarm-0.7.0-linux-x86_64.tar.gz
sh ./swarm-0.7.0-linux-x86_64/swarm-package install ./swarm-0.7.0-linux-x86_64
```

The path appears twice on the last line because `swarm-package` lives inside the
release: once to run it, once to tell it what to install.

For a later version, change the number in all three places. To always take the
newest without looking it up:

```
curl -fsSL https://api.github.com/repos/miopea/swarm-next/releases/latest \
  | grep -o 'https://[^"]*linux-x86_64\.tar\.gz' | head -1 | xargs curl -fsSLO
```

Every release is listed at
**https://github.com/miopea/swarm-next/releases** if you would rather look.

It asks you to choose a token to sign in with — at least 12 characters, typed
twice and never shown on screen. Press Enter on its own and one is generated
instead.

Then it verifies the bundle's checksums, installs it, writes the systemd units,
starts the services, and **waits for the API to answer before saying it
worked**:

```
Swarm 0.7.0 is healthy at http://127.0.0.1:8766/health

Open http://127.0.0.1:8766 and sign in with the token you chose.
```

If you let it generate one, that last line tells you how to read it back
instead:

```
sed -n 's/^SWARM_OPERATOR_TOKEN=//p' ~/.config/swarm/swarm.env
```

Installing without a terminal — from a script, or the in-app updater — skips
the question and generates a token, so nothing ever waits for input that cannot
arrive.

That is the whole installation. The rest of this page is what it put on your
machine, what to do when it does not work, and how to update.

## Before you start

You need five commands on the machine. All but the first are on most
distributions already.

| Command | Why |
| --- | --- |
| `systemctl --user` | Swarm runs as user services. A logged-in systemd user session is required. |
| `sqlite3` | The Hive database is SQLite, and the installer opens it to verify backups. |
| `curl` | The installer confirms the API answers before it calls an install finished. |
| `sha256sum` | Every release carries checksums, and they are verified on install. |
| `sed` | Reading and writing the configuration file. |

On Debian or Ubuntu: `sudo apt install sqlite3 curl`.

You also need at least one **provider** installed and working as your user —
Claude Code or Codex. Swarm does not install one, does not manage its
credentials, and cannot start a worker without one. Check with `claude --version`
before going further.

### On WSL

WSL needs systemd turned on before any of this works, and it is off by default
on older installs. Check:

```
systemctl --user status
```

If that reports that systemd is not running, add this to `/etc/wsl.conf`:

```
[boot]
systemd=true
```

then run `wsl --shutdown` from Windows and start the distribution again. Without
it there is no user session, and the install will fail when it tries to start
the services.

The browser runs on Windows while Swarm runs inside the distribution.
`http://127.0.0.1:8766` normally works from Windows unchanged, because WSL2
forwards localhost. If it does not, `hostname -I` inside WSL gives the address
to use instead.

### If the machine logs you out

User services stop when your session ends, unless lingering is enabled:

```
sudo loginctl enable-linger "$USER"
```

Without it, Swarm stops when you log out and starts again when you log back in.
That is a legitimate way to run it; it is only a surprise if nobody said so.

## What it put on your machine

```
~/.local/lib/swarm/       the installed releases and the `current` link
~/.local/state/swarm/     swarm.sqlite3 — your Hive: tasks, workers, decisions
~/.config/swarm/          swarm.env — your configuration and operator token
~/.local/bin/             swarmctl
~/swarm-workspaces/       where repositories live, unless you set another root
~/.config/systemd/user/   the ten units below
```

Everything is named `swarm`, including when you installed from a directory
called something else. The release directory's name does not survive the
install.

### The services it runs

```
swarm.target                      what you start and stop; pulls in the two below
swarm-api.service                 the application and the web UI
swarm-terminal-host.service       the terminal engine the workers run under
swarm-host-reconcile.service      applies a worker engine update
swarm-host-reconcile.path         runs it when you ask for one from the UI
swarm-host-reconcile.timer        runs it when no worker is running
swarm-development-reload.service  rebuilds a working copy (developer mode only)
swarm-development-reload.path     runs it when you ask for one from the UI
swarm-release-apply.service       installs a release you accepted
swarm-release-apply.path          runs it when you press Install
```

The last pair exists because installing restarts the API, so the API cannot do
it: the request is written to a file and a separate unit does the work.

The API and the terminal engine are deliberately separate services. That is why
updating the app does not stop your workers, and why updating the engine does.

Nothing is written outside your home directory, and nothing needs root except
the optional `enable-linger` above.

## First run

Open **http://127.0.0.1:8766** and sign in with the operator token that the
installer generated:

```
grep SWARM_OPERATOR_TOKEN ~/.config/swarm/swarm.env
```

That token is the only credential. It is generated from `/dev/urandom` at
install time, stored with `0600` permissions, and never leaves the machine.
Treat it as a password: anyone with it can drive your workers.

It is a **machine credential, not a login**. One Hive belongs to one person, and
Swarm records no per-person identity — there is no "who answered this" beyond
"the operator". If two people share a Hive they share an account in every sense
that matters, and the history will not tell them apart. Run one Hive each.

Then, in order:

1. **Settings → Crew** — add a worker. It needs a name and a repository path.
2. Wake it from the **Workers** screen. Its terminal opens in the browser.
3. **Tasks** — write a task, assign it to that worker.

The worker receives the task as a briefing in its own terminal. You do not have
to type it in.

## Connecting Jira and email

Both are optional, and Swarm works without either. They are how work arrives
from outside, so most people want at least one.

### Jira

**Settings → Integrations → Bring Jira into your Hive.** Connect with your own
Atlassian identity — project discovery uses it, so you see exactly the projects
you already have access to and nothing else.

Then bind a project. Issues assigned to you appear as tasks within a minute,
comments flow both ways, and closing an issue in Jira moves the linked task to
Completed. Each bound project is a shared ticket pool that you or Queen route to
the right repository worker.

A Jira state your binding has no mapping for holds the write rather than
guessing. Swarm tells you rather than inventing a transition.

### Email

**Settings → Integrations → Email.** A message becomes a task; the worker does
the work and drafts the reply; **you review the words and send it.** Swarm never
sends mail on your behalf without you reading it.

Several messages from one person on one thread merge into a single task, and are
answered once on the thread they wrote in most recently — not once per message.

Drafting requires the task completed and its deployment recorded, which is the
order that stops a reply going out before the change is actually available.

## Reporting a problem

**Settings → Diagnostics → copy report.** It is previewed before you copy it and
contains no terminal text, task content, workspace paths, credentials or raw
errors — so it is safe to paste into a ticket or a chat.

That report plus what you were doing is enough to act on. If a service is
involved, `journalctl --user -u swarm-api.service -n 50` adds the rest.

## Updating

Swarm can tell you a release exists and install it for you, or you can install
one by hand. Both are below.

### Letting Swarm check

The first time you open **Settings → System** you are asked once whether to
check daily. Until you answer, this Hive contacts nothing — and if you say no
it never does.

A check fetches one small signed file and compares it locally. **Nothing is
sent**: not your version, not your Hive's identity, not how many workers you
run. There is no account and nothing to sign in to.

When a release is offered you get two separate buttons, because downloading is
reversible and installing is not.

**Download** fetches the release and checks it against the digest the signature
covers. Anything that fails is discarded rather than installed and rolled back,
and a download is tied to the digest it came from — if the release it belongs to
is replaced, it is fetched again rather than installed under a version number
that has moved on.

**Install** asks to confirm first, and says what it costs. Then it runs on its
own: the card shows the install proceeding with a running count, Check now and
Stop checking lock while it does, and **the page reloads itself** once the new
App and API is healthy. You do not have to refresh, and you do not have to watch
it.

Your workers keep running throughout. Swarm restarts the API and leaves the
terminal engine they are attached to alone.

### Your workers and the engine

Most releases change only the app. When the terminal engine is unchanged — which
Swarm decides by fingerprinting the engine's own source, not the release number
— the engine simply moves to the new release and nothing restarts. You are not
asked, because there is nothing to decide.

A release that genuinely changes the engine is different. That restarts loaded
workers, and it is deferred while any worker is running: Swarm applies it once
they are idle, or when you ask for it from the worker engine card. Either way it
says so before you agree to anything.

### A Hive built from a working copy

Swarm will not offer a release to one. It says a release exists and stops there,
because replacing a binary built from your checkout would discard work nothing
can enumerate. Your updates come from the **App and API** card, which rebuilds
the checkout.

If you never turn checking on, none of this happens and the commands below are
the whole story.

### Installing a release by hand

The same three commands as installing, with `update` instead of `install`:

```
curl -fsSLO https://github.com/miopea/swarm-next/releases/download/v0.7.0/swarm-0.7.0-linux-x86_64.tar.gz
tar -xzf swarm-0.7.0-linux-x86_64.tar.gz
sh ./swarm-0.7.0-linux-x86_64/swarm-package update ./swarm-0.7.0-linux-x86_64
```

You do not need to uninstall first, and it does not matter where you unpack the
tarball. The release is copied into `~/.local/lib/swarm/releases/` and the
`current` link is moved to it, so Swarm always runs from the same place
regardless of where you downloaded it. The old release is kept for rollback.

An update keeps your database, your configuration and your workers. It verifies
the new release's health before retiring the old one, and **restores the
previous release automatically if the new one does not answer**.

The engine behaves exactly as it does through the buttons, described above: it
moves with the app when it is unchanged, and a real engine change is deferred
while sessions are running. When it defers, the command says so:

```
swarm-package: terminal host update deferred; 1 sessions remain
```

Apply it when you are ready, from the worker engine card or by running the
update again once workers are asleep.

## Building it yourself

Only if you are working on Swarm rather than using it. You need a Rust
toolchain and `pnpm`, and the build takes a few minutes.

```
git clone https://github.com/miopea/swarm-next.git swarm
cd swarm
./packaging/linux/build-development-release.sh /tmp/swarm-build
sh /tmp/swarm-build/swarm-*/swarm-package install /tmp/swarm-build/swarm-*
```

A build made this way is a **development build**, not a release. Swarm will tell
you when a release exists but will never offer to replace it, because replacing
a binary built from your checkout would discard work nothing can enumerate.

### Turning the rebuild into a button

If you are working on Swarm itself, the rebuild-and-update loop above can be a
button in the control room instead. Point Swarm at your checkout once:

```
sh ~/.local/lib/swarm/current/swarm-package enable-development ~/swarm
```

From then on **Settings → System** carries an *App and API* card that rebuilds
that checkout and activates it, and the runtime area says this Hive builds from
a working copy. Without this step there is no App and API update control in the
interface at all, which is correct — there is nothing local for it to build.

`disable-development` returns you to installed releases. This is a developer
convenience and not how Swarm is meant to be run.

## If an update goes wrong

```
sh ~/.local/lib/swarm/current/swarm-package rollback
```

Returns to the previous release. Your database is untouched by a rollback —
schema changes are forward-only, so rolling back the binaries does not roll back
your data.

To restore a database from a backup:

```
sh ~/.local/lib/swarm/current/swarm-package restore /path/to/swarm.sqlite3
```

The backup is put through a full integrity verification before anything is
replaced, and the database it displaces is kept until the new one is in place.

## Removing it

```
sh ~/.local/lib/swarm/current/swarm-package uninstall
```

Stops and removes the services and the installed application. **Your
configuration and your Hive database are deliberately left behind**, in
`~/.config/swarm` and `~/.local/state/swarm`. Delete those yourself if you mean
to lose the data; the uninstaller will not do it for you.

## When it does not work

**The install says the API did not answer.** The services are installed but
something stopped them starting. `systemctl --user status swarm-api.service`
names the reason; `journalctl --user -u swarm-api.service -n 50` shows it.

**You cannot reach the web page.** Swarm binds `127.0.0.1:8766` on purpose, so
it is reachable only from that machine. Reaching it from a phone means putting
your own HTTPS proxy in front of it — Swarm does not open itself to the network.

**A worker will not start.** Almost always the provider: check `claude
--version` as the same user. Settings → Diagnostics names which layer is
unhappy, and says so in one line rather than fourteen.

**Something else.** Settings → Diagnostics has a report you can copy. It is
previewed before you copy it and contains no terminal text, task content,
workspace paths, credentials or raw errors.
