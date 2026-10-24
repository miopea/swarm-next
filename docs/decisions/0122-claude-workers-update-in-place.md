# 0122 — Claude workers move onto a new Claude release in place

- Status: Accepted
- Date: 2026-10-23
- Decided by: the operator, in the Swarm Next worker's terminal on 2026-10-23:
  - Introducing it: "there is a /restart command now that can restart workers
    more cleanly than sleeping and waking. Ideally this is how we keep it
    update, if a worker is idle then we just run the command, if they are
    doing something we run if (if not shells are running) and when it conmes
    back tell it to continue where it left off".
  - Asked when Swarm should run it, they chose "Newer Claude installed" and
    "Only when I press Restart", adding: "it should ONLY run if there are
    updates, not in mass like now because this allows workers to keep
    updating. But updates should be much more automatic this way too."
  - Asked when to send the carry-on message: "Only if it had work going".
  - On Remote Control, after Claude refused `/restart` with it connected: "I
    actually would rather /rc be turned off anyway, it isn't useful when I have
    swarm", then "We should probably set this in settings to turn on /rc or not
    and what it means if you do."
  - Asked whether to build it on the terminal host, accepting one worker
    engine update before it works: "Yes, via the host".
- Builds on: the provider release tracking in `swarm_terminal::provider`
  (`provider_release`, `provider_release_superseded`), and the coordination
  delivery gate (`submit_coordination_message`).

## Context

Claude updates itself on disk, and a running worker keeps executing the
release it started with. Swarm already reported workers on a superseded
release, and its only remedy was to stop each one and start it again from its
saved conversation. Running work was interrupted, and the operator then had to
tell every worker to continue, by broadcast.

Claude Code now has a `/restart` command, also named `/update`. It is not in
the docs or the changelog; it is defined in the installed binary as "Restart
Claude Code and keep this session", behind a feature gate. Measured on
2026-10-23 in a throwaway terminal, starting the session on 2.1.309:

- The process id stayed the same. `/proc/<pid>/exe` moved to 2.1.310.
- The process came back as `--resume <same session>`, with the same
  `--settings`, `--mcp-config` and `--strict-mcp-config`.
- It remembered a word from before the restart.
- Claude reported `SessionStart` with source `resume`, and no `SessionEnd`.
- It refuses while background work is running, while Remote Control is
  connected, and for a session not saved to disk.

The operator's own broadcast of `/restart` did nothing. Claude runs a command
only when a message begins with it, and a broadcast begins with its header.

Because the process and its start time stay the same, nothing Swarm recorded
could tell an updated worker from one still behind. Judged by start time, a
worker that had just updated would read as superseded for as long as it lived,
and would be asked to restart again every time it rested.

## Decision

### 1. The host reports the executable each terminal is running

`HostSessionSummary` gains `provider_executable`, read from `/proc/<pid>/exe`.
It is a defaulted field, so there is no protocol change, but it needs the
worker engine update before any session has one.

`provider_release_superseded` uses it whenever it can be compared, and falls
back to start time when it cannot. It can be compared only when it sits beside
the installed release in the same versions directory, which is the layout
Claude's installer uses. A launcher script or a package-managed binary has a
running image that is not the release file. Comparing it would report a
difference no restart can remove.

Linux marks an image whose file has been removed with " (deleted)". The path
before it still names the release the process runs, because Claude prunes old
releases while long-lived workers still run them. A deleted image is never the
installed release, even at the same path: an installer without versions in its
paths replaces the file in place, so an older process reports the very path now
installed.

### 2. Swarm types `/restart`, one worker at a time, as each comes to rest

Every supervisor pass looks at each live Claude worker. The step is decided by
`update_step` from observations only:

| Situation | Step |
| --- | --- |
| On an older release, resting, nothing running in the background | Type `/restart` |
| On an older release but working, asking a question, or with shells, monitors or a workflow running | Wait |
| Already asked onto this release, still behind | Wait (asked once) |
| Asked, and now on the installed release, with Active work | Carry on |
| Asked, and now on the installed release, without Active work | Forget the request |
| No executable, or one that cannot be compared | Wait, and leave it to stop-and-start |

`/restart` goes through `submit_coordination_message`, the gate every
coordination delivery uses. It writes only into an empty resting prompt, never
over unsent text or a question, and never while an interactive view holds the
terminal.

The carry-on message names the release. It asks for nothing new, so a worker
that was waiting on someone keeps waiting.

### 3. One ask per release

A worker still behind after its `/restart` most likely had Claude refuse it,
over Remote Control or background work. Asking again every time it rests
would be a loop, so each release is asked for once per session. The Updates
card says which workers Claude refused, and its button asks them again. A
newer release is a new question and gets its own ask.

The requests are held in memory. An API restart forgets them, which costs at
most one more `/restart` for a worker Claude refused. It can also cost a
carry-on message for a worker that is resting anyway. Neither is worth a
table.

### 4. Only what Swarm cannot do asks the operator

The Restart button no longer stops a Claude worker Swarm can move in place.
For those workers it clears the ask and runs the pass at once. Codex workers,
and Claude workers whose release the host cannot read, keep the
stop-and-revive they had.

The rail's provider notice follows the same split:

- When every worker behind can be moved in place, the notice is one line with
  nothing to press: "Updating workers", "Automatic, as each comes free".
- A refused worker brings back an **Ask again** button, which carries no
  warning because it interrupts nothing.
- Workers that need stopping keep the notice and the warning they had, counted
  without the ones Swarm moves itself.

The notice is a one-line row, not a spinner card, though it is work in
progress. A worker can stay busy for hours, and a full card held that long is
what left the worker list at nothing on 2026-10-12.

### 5. Remote Control for workers is the operator's choice, off by default

Schema 209 adds `worker_remote_control_settings`. Settings → Workers has
**Let Remote Control connect to workers**, which explains what turning it on
costs: Swarm cannot update a connected worker.

The API writes `remoteControlAtStartup` into the worker's settings file under
its own key, beside `permissions` and `model`. It is written either way,
because Claude's own default connects workers. The host copies it into the
startup settings Claude launches with. It reaches each worker the next time
that worker starts.

## Consequences

- None of this acts until the worker engine update. Before it, no session has
  an executable, so every worker stays on the stop-and-start path and the
  Remote Control choice is not written into the startup settings.
- The worker engine update itself restarts every worker, and they come back on
  the current Claude with the Remote Control choice applied. Claude updates
  after that reach each worker in place.
- A worker is never interrupted for an update. One that stays busy stays on the
  older release until it rests.
- Claude can refuse a restart for reasons Swarm does not see, such as a running
  subagent. Such a worker is reported as refused and waits for the button.
- `/restart` is undocumented. If Claude changes it, the one-ask rule bounds the
  damage to one ask per release, and the card shows the workers still behind.
