# Release notes

What each release means, written for the people running Swarm rather than for
the people who wrote it.

`swarm-release notes` prefers a section here over the commit subjects, per
release. A version with no section falls back to generated notes, and the build
says on stderr which source each release used — so notes written for a version
that never gets cut are noticed rather than silently dropped.

Format: `## <version>`, then `### New features` and `### Fixes`, then `- ` bullets.
End a bullet with `(after the worker engine update)` when it is installed but
not in effect until the worker engine swaps.

## 0.9.2

**Take this if 0.9.0 or 0.9.1 would not install for you.** Both of them could
fail on a machine where stopping Swarm takes a moment: the update carried on
before the old worker engine had actually shut down, the new one could not talk
to it, and after four minutes the update gave up and rolled itself back. It
reported the failure as a health check problem, which it never was — so nothing
on screen pointed at the real cause.

**It still restarts your workers**, like 0.9.0 and 0.9.1: the app and the worker
engine have to move together. Workers set to start automatically come back on
their own; the rest need waking from the roster.

**Copy your database first.** This carries schema migration 103:

    cp ~/.local/state/swarm/swarm.sqlite3 ~/swarm-database-backup.sqlite3

### Fixes
- An update now confirms Swarm has actually stopped before replacing it, and
  asks again if something has not shut down, rather than assuming and carrying
  on. This is what stopped 0.9.0 and 0.9.1 installing
- A failed update no longer blames the health check for something else going
  wrong, so the message points at the step that actually failed
- The worker engine card no longer reports the engine as current when it cannot
  reach it at all — an unreachable engine now says so

## 0.9.1

**Take this instead of 0.9.0.** It is the same update and it finishes. On a
slower machine 0.9.0 ran out of time waiting for the API to come back, called
the install failed, and rolled itself back — correctly, but repeatedly, so the
update could not be completed at all.

**It still restarts your workers**, for the same reason 0.9.0 did: the app and
the worker engine have to move together. Workers set to start automatically
come back on their own; the rest need waking from the roster.

**Copy your database first.** This carries schema migration 103:

    cp ~/.local/state/swarm/swarm.sqlite3 ~/swarm-database-backup.sqlite3

### Fixes
- An update that changes the worker engine no longer gives up while the app is
  still starting. It used to allow thirty seconds for a step that stops
  everything, swaps both halves, restarts them and migrates the database — then
  report a working install as failed and tell you to read the system log
- The Reload button on the "running an older version" notice now actually loads
  the new version, instead of leaving you on the old page until you did a hard
  refresh yourself
- The bee picker no longer squashes the worker name field into a sliver when
  you edit a worker
- What is New is wider on a desktop screen, where it read as a narrow strip

## 0.9.0

**This update restarts your workers.** It changes the protocol the app uses to
talk to the worker engine, and those two have to move together, so every worker
session ends and starts again. Workers set to start automatically come back on
their own; the rest need waking from the roster. Nothing else in Swarm does
this — ordinary updates leave your terminals alone.

**Copy your database first.** This carries a schema migration, and the backup
that protects a local rebuild does not cover a downloaded release:

    cp ~/.local/state/swarm/swarm.sqlite3 ~/swarm-database-backup.sqlite3

### New features
- Every worker wears a different bee, so one repository's worker can be told
  from another's at a glance. Assigned automatically and changeable per worker
  on the worker page — 23 to choose from
- The control room's own bee is the Queen
- Gemini, Grok and OpenCode can be chosen when creating a worker, labelled
  alpha where you pick them (after the worker engine update)

### Fixes
- A failed update now tells you which step failed and quotes what the failing
  tool actually said, instead of one sentence that was the same whatever went
  wrong. A build that compiled fine and was refused at install used to report
  "did not compile"
- The install card no longer claims "nothing was changed" without checking. It
  reads back what actually happened and says so, including when an install
  stopped partway
- An update that changes the worker engine protocol now installs in one step.
  Before, a Hive would refuse it with no way forward and the only route was
  editing files by hand
- A refused install names the command that fixes it rather than describing one
- A failure whose fix arrived another way stops being reported, so the control
  room no longer offers you a build of the code it is already running
- A pre-update database backup that cannot be verified now says what to do next
- The update no longer prints anything from the one request that carries your
  operator token

## 0.8.19

### New features
- The What's New panel now shows everything since the release you actually
  updated from, so opening the control room on a second machine no longer
  looks like a first install

### Fixes
- A Hive with Outlook registered and no public address starts again, instead of
  exiting on boot and being restart-looped
- A failed update can be recovered from with ordinary commands. The pre-update
  backup no longer needs the API it is protecting, a rollback restores your
  database rather than leaving it ahead of the release, and a service that has
  been crash-looping is reset instead of refusing every later install
- Update checks report the engine your Hive is running, not the one it was
  installed from
