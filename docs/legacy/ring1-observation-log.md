# Ring 1 observation log

Status: **Active; formal evidence window opened 2026-08-17**

This log measures the bounded real-use companion to the Legacy atlas. It does
not count automated tests as operator-use hours and does not manufacture Jira,
email, Apiary, Queen, or worker effects merely to populate a checklist. Seven
days are not complete until seven elapsed 24-hour periods have been observed.

## Bounds

- Queen automatic operation remains off unless the operator explicitly enables
  a bounded journey.
- Real Jira, email, task, worker, and Apiary mutations occur only through normal
  operator work or a separately authorized proof.
- Browser proof uses private disposable tabs, records content-free evidence,
  and closes every test tab afterward.
- Legacy remains read-only. Remote-head checks and commit inspection may update
  no Legacy product files, database, configuration, or running process.
- A quiet day is evidence: absence of reconnect storms, unexplained writes,
  worker resurrection, memory growth, and notification noise should be recorded
  rather than replaced with synthetic activity.

## Evidence to capture

For each naturally occurring session, record only what is relevant:

1. active App/API and worker-engine identities;
2. operator journey and viewport/device class;
3. worker loaded/sleeping ownership and resource evidence when material;
4. terminal geometry, reconnect, scroll, focus, paste, or provider-question
   behavior when exercised;
5. task/Jira/email/Apiary convergence only when ordinary work uses it;
6. incident, recovery path, and whether refresh actually repaired the state;
7. any Legacy overlap and its atlas classification; and
8. whether external state changed.

## 2026-08-17 — opening evidence

### Wide desktop terminal geometry

- The installed Scout terminal visibly occupied the browser surface while the
  live provider PTY remained `7 x 50`; resize and refresh did not repair it.
- The failure mapped to Legacy's stable resize/reconnect outcome but had a Next
  cause: the browser accepted a usable transitional fit and suppressed the later
  settled geometry.
- `c27ecc5` requires two stable fit frames and republishes the final visible
  geometry even when xterm's own row/column count is unchanged. The exact
  `7 x 50` to `42 x 168` sequence and the full frontend gate pass.
- The operator approved the intentionally disruptive worker-engine restart.
  Protocol 9 was installed from release
  `0.1.0-dev-ba284f1f7e49-20260817111126-1078561`; Queen revived from its saved
  provider conversation and the manually awake, non-always-active Scout
  correctly remained sleeping.
- The live Queen terminal then measured 1,052 by 738 CSS pixels inside a
  1,440 by 900 desktop viewport, 398 by 576 inside an Android-size 412 by 915
  viewport, and exactly 1,052 by 738 again after returning to desktop. Both
  surfaces had zero horizontal page overflow, the browser emitted no warnings
  or errors, and the proof tab was closed. This closes the observed
  mobile-to-desktop terminal-geometry defect.

### Sleeping-worker ownership and resource attribution

- The UI showed two awake workers out of 30 configured.
- The Next worker-engine cgroup owned exactly two provider processes—Queen and
  Scout—and no process for the 28 sleeping workers.
- Extra Claude processes belonged to Legacy's separate service cgroup. The
  simultaneous service snapshots are preserved in
  `../24-browser-dogfood-acceptance.md`; they prove ownership and unload behavior,
  not a normalized per-worker memory comparison.

### Atlas and safety state

- A direct remote-head check still returned Legacy `main` at `1f559e84`, matching
  the 1,529-entry ledger.
- Queen automatic operation remained off, no real Jira, email, task, worker, or
  Apiary mutation was used for this evidence, and proof browser tabs were closed.
- The operator accepted all four recommended Ring 1 boundaries: Queen remains
  notify/recommend-only at provider questions; PTY write evidence remains
  diagnostics-only; repository-specific completion checks remain repository
  owned; and configurable worker command macros remain deferred while real
  repetition is measured.
- The operator also selected transparent, developer-specific learning as an
  evidence-backed opportunity. Queen may propose a narrowly scoped preference
  from repeated corrections, but it remains inert until approved; the active
  ruleset must be visible, editable, and removable in Settings and may not
  silently expand authority or become shared Apiary policy.
- Queen-discovered Routines follow the same boundary. Queen may surface a
  repeated multi-step journey in Settings, but the operator reviews and
  controls its definition and activation; typed deterministic steps execute
  below Queen while judgment and external authority remain explicit.
- The protocol migration left API and holder builds aligned on protocol 9,
  both services active with zero restarts, database `quick_check` at `ok`, and
  the holder reporting one running and one retained Queen session. No Jira,
  email, task, or Apiary mutation was used for the proof.

## Window closure test

At the end of the seventh elapsed day:

1. refresh the Legacy remote head and regenerate the ledger if it moved;
2. summarize incidents, quiet periods, recoveries, and repeated operator
   friction without promoting one-offs by commit count;
3. compare each new material observation with a current Next owner and test;
4. update the decision packet only where evidence changed a recommendation; and
5. rerun `atlas-completion-audit.md` before claiming the objective complete.
