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

## 1.2.1

**Fixes a 1.2.0 defect that leaves the worker engine stuck.** No schema change,
no protocol change. If your engine card says "Update ready · restart required"
and pressing the button spins and then reverts, this is the release that fixes
it.

### Fixes

- The worker engine update works when your running engine is too old to report which sessions are busy — which is exactly the case on the first upgrade to a build that reports them. The timer deferred and the card refused, so the engine could not update by any route, and nothing said why. The card now proceeds, saying loudly what it could not verify, which is what it always claimed to do.

## 1.2.0

**This one migrates your database (schema 113 to 119) and needs your workers to
reconnect. Copy `swarm.sqlite3` before installing — the automatic pre-update
backup covers a reload from a checkout, not a tarball install.** The protocol is
unchanged, so the install itself leaves running workers alone.

### New features

- Finished work that has shipped nothing yet rests in **Awaiting Release** instead of sitting in Review pretending to need a decision. It closes itself when a deployment is recorded, with nobody clicking.
- Queen and a worker can now message each other about a task, and the exchange is kept on the task rather than lost in a terminal. A worker can raise something unprompted, not only answer.
- **Tell every worker** in the header says one thing to every running worker at once. It waits until each terminal is resting, and it reports how many it reached and how many had no live session — so you are never told everyone heard you when they did not.
- **Queues** groups open work by who owes the next move, so a growing pile is attributable instead of anonymous.
- **Force worker reload** on the worker engine card reconnects every worker. Needed after an update that changes an agent tool, which the engine status cannot show you.
- The worker engine card now warns when live sessions are holding an older tool list than this build serves, including when it cannot confirm what they hold.
- Recording a deployment can say it delivered only PART of a task, so shipping half of something no longer closes the whole ticket.

### Fixes

- Messages from Queen actually submit. They were being typed into the worker's prompt and left there unsent, while the board recorded them as delivered.
- A worker's reply to Queen is delivered at all. The channel was one-way, so replies sat unread for up to 76 minutes.
- The busy-worker refusal names which task is holding the slot instead of leaving you to guess from the board.
- The worker engine card says which build is **Running** rather than labelling it "Installed", which is the one thing it certainly was not.
- The board no longer re-sends every task on every change. It carried 1.7 MB per refresh and now carries 188 KB, and completed work is no longer rebuilt behind a collapsed panel.
- Dropping a docx, an mp4 or any other non-image file leaves a visible reference in the prompt instead of a bare space that looked like nothing had happened.
- The worker engine fingerprint notices changes in the shared crates the engine actually links, so an engine update is no longer missed. Expect one engine restart on this release for that reason alone.

## 1.1.2

**No schema change, no protocol change, and your workers keep running.**

### New features

- **Reporting a bug about Swarm no longer needs any setup.** Filing feedback
  used to work only if whoever installed Swarm had gone and got a GitHub token
  of their own — and until they did, the dialog quietly offered only "Save to
  this Hive", which looks like a choice rather than an install that cannot
  reach the project. A release now carries its own credential for the Swarm
  repository, so a fresh install can file straight away. Setting
  `SWARM_GITHUB_REPOSITORY` and `SWARM_GITHUB_TOKEN` still wins if you want
  your own destination, and reports from your Hive stop going out under
  whoever's token happened to be configured
- **A Swarm that genuinely cannot file now says so**, in the dialog, before you
  write the report rather than after

### Fixes

- **Attaching a file tells you it worked, and which file.** The confirmation
  existed but had no styling at all, so it rendered as grey text in a busy
  toolbar — a file that attached correctly looked exactly like one that had
  done nothing. It now reads as "Added yourfile.mp4 · press Enter to send"
- **A dropped video, tar or gzip arrives as what it is.** Anything Swarm did
  not recognise was stored with a `.bin` name, so a worker was handed a file
  with no hint what it held. This does not let a worker watch a video — it
  stops the file arriving anonymous
- **A Word document or PowerPoint deck is no longer stored as a spreadsheet.**
  All three Office formats were being written with an `.xlsx` name, which fails
  later looking like a corrupt file rather than a mislabelled one
- **The "waiting on you" card on Needs you can be read.** It now says whose work
  each item is, groups by worker, and puts the reason in a short label instead
  of repeating the same sentence down the list. Eleven items used to wrap into
  a ragged column with nothing to scan

## 1.1.1

**An easy one: no schema change, no protocol change, and your workers keep
running.** Everything here is the phone and the feedback dialog.

### Fixes

- **Attaching a file on a phone stops failing silently.** Opening the picker
  puts this page in the background, which drops the terminal connection — and
  the attachment was then discarded without a word, so it worked or it did not
  and nothing said which. Uploading never needed that connection: the file now
  uploads regardless and is added the moment the terminal is back
- **A file too large to send says so before uploading it**, naming the file and
  the limit, instead of running the upload to the end and failing with nothing
  legible. This is what a phone video usually does — one minute is commonly ten
  times the limit
- **The phone stops offering to attach a video.** It never could use one, and
  the picker was simply not saying what it takes. Photos, logs, CSVs, PDFs and
  the rest are unchanged
- **A picker that comes back with nothing now says so.** Occasionally a phone
  discards the page while its file picker is open, and nothing arrives; that
  used to look identical to a working app doing nothing
- **The feedback dialog stops changing its own button.** It could read "Save to
  this Hive" and then become "Send to GitHub" a moment later, while the dialog
  worked out whether a GitHub account was connected. Those are different
  destinations and it should never have claimed one before knowing. It now says
  it is still checking
- **One button in that dialog is clearly the main one.** Two of the three were
  competing for it

## 1.1.0

**Your workers keep running through this one.** The terminal protocol did not
move, so this installs with `update` and leaves live sessions alone.

**Copy your database first.** This carries schema migrations **110, 111 and
112**, and two of them rebuild a table rather than adding one — including
`tasks`, which forty-three other tables point at. A tarball install migrates
without taking its own backup:

    cp ~/.local/state/swarm/swarm.sqlite3 ~/swarm-database-backup.sqlite3

The upgrade has been run on a real database with real work in it — 409 tasks
and 3102 activity records came through unchanged — but yours is not that one,
so take the copy.

### New features

- **Work you gave up on can be closed as abandoned.** Until now the only way to
  close anything was to complete it, which asked what evidence showed the work
  was running — a question that has no answer for work nobody finished, and one
  you were answering by hand. Abandoned never asks it
- **Finished work that had nothing to deploy now closes itself.** An
  investigation that produced no commits, or a change that only touched
  documentation, is settled by Swarm on the facts rather than waiting for you to
  confirm it
- **A worker records which commits its task produced**, and Swarm checks them
  against your repository — whether each one exists, whether anything still
  reaches it, and what it touched. That is what makes "there was nothing to
  deploy" something Swarm can establish rather than something it is told
- **Needs you now says how much finished work is waiting on you**, and why each
  piece waits: it built code and recorded no deployment, nobody reported what it
  produced, or somebody claimed nothing shipped and no one has agreed
- **A claim that nothing was deployed is refused when the commits disagree**,
  and the refusal says what to do instead

### Fixes

- A worker whose terminal could not be read is no longer reported as a worker
  with nothing running. Those are different things, and saying the second when
  the first is true sent people to check on workers that were perfectly fine

## 1.0.2

**Workers keep running through this one**, and there is no schema change.

### New features
- What's New can show you the releases before the one you just took. An
  **Earlier releases** button under the new notes opens the rest of the history
  your Hive already carries, each labelled with its version
- **Settings → Updates → Release notes** opens that same panel whenever you
  want it. Until now What's New appeared once after an update and was gone for
  good the moment you dismissed it

## 1.0.1

**Workers keep running through this one**, whichever version you are coming
from, and there is no schema change.

### Fixes
- What's New shows the release notes as they were written. 1.0.0 was the first
  release whose notes used bold and inline code, and the panel printed the
  asterisks and backticks instead of applying them
- The What's New panel is much wider. It asked to be 760px and had been
  overridden to 440px since the day it was written, so it has always been
  narrower than intended — it is now up to 980px, and still fits a phone
- Emphasis in that panel is now heavy enough to actually read as emphasis

## 1.0.0

**Whether your workers survive this depends on where you are coming from.**
Check which you are on:

    cat ~/.local/lib/swarm/current/VERSION

- **From 0.9.x — your workers keep running.** The terminal protocol did not move
  since 0.9.0, so live sessions are left alone.
- **From 0.8.x — every worker session ends.** You are on the older terminal
  protocol, and the app and the worker engine have to be swapped together. The
  command is the same; there is nothing extra to run. Workers set to start
  automatically come back on their own, and the rest need waking from the roster.

**Copy your database first.** This carries schema migrations 104 through 109 —
six of them, the largest jump any release has asked for. A tarball install
migrates without taking its own backup:

    cp ~/.local/state/swarm/swarm.sqlite3 ~/swarm-database-backup.sqlite3

### New features

- **Feedback goes to GitHub.** The feedback dialog now files a real issue on
  the repository, and tells you where it went — including when it could not get
  there, instead of silently keeping it local
- **Connect your own GitHub account** from the feedback dialog itself, so the
  issue is filed as you and the answer comes back to you when it is closed.
  Connecting takes a code and a browser tab; nothing is typed into Swarm
- **Without a connection you still get to file.** The issue goes up anonymously
  and says so in its own footer, so nobody waits for a reply that has no address
  to arrive at
- **A connection that lapsed renews itself** in the background, and says plainly
  when it cannot rather than failing at the moment you try to use it
- **Issues come back as work.** An issue opened on the repository arrives as a
  draft task for the queen to judge, rather than needing to be copied across by
  hand
- **The phone can wake and sleep a worker**, and a worker with a wake already in
  flight says so instead of looking dead for four minutes
- **Queen can start a worker directly**, and an assignment that reached nobody
  now says that it reached nobody
- **The control room says when the machine itself is in trouble** — a
  CPU-saturated box no longer reports itself as normal, and a start refused for
  pressure names which pressure refused it
- **Finished work can be recorded as unverifiable** from the panel that asks
  about it, so work that genuinely cannot be verified stops accumulating as an
  open question
- **An outside tool can register, be approved, and be issued a token**, arriving
  on the board as itself so its work is attributable

### Fixes

- Importing email as tasks works again. A long subject line no longer blocks a
  task you titled correctly, an email carrying the same attachment twice no
  longer fails the whole import, and removing a task releases the emails it was
  holding instead of stranding them
- A store failure is reported as what it was. The blanket "temporarily
  unavailable" that swallowed every underlying error now records the actual
  cause
- Swarm no longer closes email work that owes somebody a reply, and the check
  that was supposed to guard sending a reply now guards the send rather than the
  draft box
- A task with no email thread says exactly that, instead of blaming its own state
- Refusals name the caller's real problem in twenty-five places that previously
  reported the wrong reason — most visibly, work you cannot see is reported as
  not yours rather than as missing
- **A page load stops replaying the whole event history.** Opening the control
  room on a phone read thousands of entries to show the newest sixteen, which is
  the reason it felt slow and needed a refresh
- A terminal that went quiet while its tab was frozen in the background is now
  replaced when you come back, instead of looking connected and being dead
- A phone no longer resizes or shreds a terminal another device owns, and a
  desktop reopened after a phone session takes its own width back
- The collapsed worker picker keeps one consistent second line and no longer
  throws while drawing, a worker on an active task reads "Working" rather than
  "Resting", and the switcher says which row you are on instead of tinting it
- "Needs you" reads as one column on a phone, ranks what it shows, and its reply
  box is readable without scrolling
- The terminal's Refresh button is called Refresh, and Redraw sits beside Add
  file rather than inside the panel it hides
- The MCP endpoint accepts requests arriving through the tunnel, its 401 says
  where to authenticate, and the consent button works — its redirect was being
  blocked outright

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
