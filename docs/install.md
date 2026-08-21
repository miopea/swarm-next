# Installing Swarm

Swarm runs as a set of systemd **user** services on one Linux machine. It is not
a server you administer and not a container you orchestrate: it installs under
your home directory, runs as you, and listens only on localhost.

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

## Getting a release

Everything below installs a **release directory** — a folder containing `bin/`,
`web/`, `systemd-user/`, `swarm-package`, and the `VERSION`, `PROTOCOL`,
`SOURCE_REVISION` and `SHA256SUMS` files that describe it. There are two ways to
get one.

### You were given a tarball

```
tar -xzf swarm-0.1.0-linux-x86_64.tar.gz
```

That leaves a `swarm-0.1.0-linux-x86_64/` directory beside it. Skip to
**Installing**.

### You are building it yourself

You need a Rust toolchain and `pnpm` as well as the commands above, and the
build takes a few minutes because it compiles in release mode and bundles the
browser assets.

```
git clone https://github.com/miopea/swarm-next.git swarm
cd swarm
./packaging/linux/build-development-release.sh /tmp/swarm-build
```

That prints nothing on success and leaves one directory under `/tmp/swarm-build`,
named for the revision it was built from:

```
/tmp/swarm-build/swarm-0.1.0-dev-<revision>-<timestamp>-<pid>-linux-x86_64/
```

That directory is the release directory. Use it wherever the commands below say
one.

> **Building a tagged release instead.** `build-release.sh` produces a
> distributable tarball under `dist/`, but it **refuses to run on an untagged
> commit** — a release version has to come from a tag so that two releases can be
> compared. Tag first (`git tag -a v0.1.0 -m "Swarm 0.1.0"`) or use the
> development build above, which is the normal thing when working from a clone.

## Installing

```
sh ./swarm-0.1.0-linux-x86_64/swarm-package install ./swarm-0.1.0-linux-x86_64
```

Substitute your own release directory — the built one is under `/tmp/swarm-build`
and has a longer name. `swarm-package` lives inside it, so the path appears
twice: once to run it, once to tell it what to install.

The installer verifies the bundle's checksums, installs the release under
`~/.local/lib/swarm/releases/`, writes the systemd units, starts the services,
and then **waits for the API to answer before reporting success**. If it does
not answer, the install does not claim to have worked.

On success it prints:

```
Swarm 0.1.0 is healthy at http://127.0.0.1:8766/health
```

### What it creates

```
~/.local/lib/swarm/       the installed releases and the `current` link
~/.local/state/swarm/     swarm.sqlite3 — your Hive: tasks, workers, decisions
~/.config/swarm/          swarm.env — your configuration and operator token
~/.local/bin/             swarmctl
~/swarm-workspaces/       where repositories live, unless you set another root
```

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

Then, in order:

1. **Settings → Crew** — add a worker. It needs a name and a repository path.
2. Wake it from the **Workers** screen. Its terminal opens in the browser.
3. **Tasks** — write a task, assign it to that worker.

The worker receives the task as a briefing in its own terminal. You do not have
to type it in.

## Updating

```
sh ./swarm-0.2.0-linux-x86_64/swarm-package update ./swarm-0.2.0-linux-x86_64
```

An update keeps your database, your configuration and your workers. It verifies
the new release's health before retiring the old one, and **restores the
previous release automatically if the new one does not answer**.

Updating the API does not stop your workers. Updating the *terminal engine*
does, so Swarm defers that while sessions are running and tells you it has:

```
swarm-package: terminal host update deferred; 1 sessions remain
```

Apply it when you are ready, from the runtime area in the control room or by
running the update again once workers are asleep.

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
