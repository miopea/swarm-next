# M1 two-worker soak gate

Status: **Implemented harness; promotion evidence pending**

M1 is not complete merely because reload and API replacement work once. The
promotion gate uses two real Claude sessions, repeated API replacement, and
content-free resource samples over 24 hours.

## Harness

`scripts/dogfood/two-worker-soak.sh`:

- starts exactly two sessions through the authenticated public API;
- verifies both immutable session IDs remain present and running;
- restarts only the replaceable API at a bounded interval;
- samples API and terminal-host memory and task counts;
- samples bounded history usage and dropped-byte counters;
- writes a timestamped CSV plus a compact pass summary;
- stops only the two sessions it created unless explicitly asked to retain
  them;
- passes the operator token through a mode-0600 temporary curl configuration,
  never a process argument or report.

Example on the dogfood host:

```sh
SWARM_OPERATOR_TOKEN='…' \
SWARM_SOAK_WORKSPACE_A="$HOME/projects/project-a" \
SWARM_SOAK_WORKSPACE_B="$HOME/projects/project-b" \
SWARM_SOAK_DURATION_SECONDS=86400 \
scripts/dogfood/two-worker-soak.sh
```

Reports default to `~/.local/state/swarm-next/soak`. The raw CSV is evidence;
passing requires no missing session, failed API call, unbounded history, or
sustained application-memory growth. Provider-process memory is reported with
the terminal-host cgroup but evaluated separately from Rust-host idle memory.

## Browser companion test

The soak is paired with browser automation against the packaged public UI:

1. attach both workers;
2. switch repeatedly without creating replacement terminal connections;
3. reload during output;
4. replace the API and observe both sessions recover;
5. verify terminal dimensions remain equal to the visible host;
6. send input to both recovered sessions;
7. stop both and verify zero retained live sessions.

The automated harness establishes duration and resource evidence. The browser
test establishes the end-user interaction contract; neither substitutes for
the other.

`scripts/dogfood/browser-memory-soak.cjs` adds the missing endurance half. It
launches one isolated headless Chromium-family browser, authenticates normally,
and leaves the live Settings surface subscribed without mutating tasks or
workers. Chrome DevTools identifies the exact browser and storage-service
processes; OS counters then sample their working set and private bytes alongside
the complete owned browser tree, renderer JavaScript heap, DOM nodes, and
browser storage usage. The browser PID is pinned and authenticated-page errors
fail the run. After a five-sample warmup, a run fails only when both material
net growth and a sustained positive slope exceed the documented bounds, so a
normal warm cache does not masquerade as a leak.
Every minute it also cycles through Workers, Tasks, and Settings using only
read operations, exercising terminal detach/reattach, control-room listeners,
and route cleanup rather than measuring a completely idle tab.

Example from Windows, where Edge was the original problem surface:

```powershell
$env:SWARM_OPERATOR_TOKEN = '<private token>'
$env:SWARM_BASE_URL = 'https://swarm2.example.test'
$env:SWARM_BROWSER_EXECUTABLE = 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
$env:SWARM_BROWSER_SOAK_DURATION_SECONDS = '86400'
node scripts\dogfood\browser-memory-soak.cjs
```

The token is read only from the environment and never written to evidence.
CSV samples and a compact JSON verdict default to `dist/browser-soak`.

The first live harness validation on 2026-08-13 ran release
`0.1.0-67d625162b60` in isolated headless Edge for 60 seconds and 13 samples.
Browser PID `49736` stayed fixed; its private memory remained between 61.5 and
61.9 MiB and ended lower than the post-warmup baseline. The storage service
remained between 9.9 and 10.0 MiB, the owned browser tree ended 22.3 MiB lower,
renderer heap ended lower, DOM nodes remained exactly 440, browser storage
remained zero bytes, and no authenticated-page error occurred. This validates
the harness and rules out violent immediate growth; it does not replace the
24-hour browser soak.

A 30-minute active browser run then pinned Edge browser PID `42888` for 60
samples while cycling Workers, Tasks, and Settings. Browser private memory
ended 0.8 MiB lower, the storage process had zero net growth, the complete
browser tree ended 15.5 MiB higher with a 0.5 MiB/minute fitted slope, renderer
heap ended 0.2 MiB higher, DOM nodes remained exactly 440, and browser storage
remained zero. A later rolling-update proof on release
`0.1.0-659c2584cd18` deliberately restarted only the API. The authenticated
browser observed ten exact gateway errors, recovered in 3.7 seconds without a
new token, completed five more navigation cycles, and passed all memory bounds.
The soak permits only that exact bounded gateway signature, requires public
health and an authenticated-only Settings control to return, and still fails
every other console or runtime error.

A later 10-minute navigation reproduction on release
`0.1.0-c836ebec1772` completed 40 samples and 19 Workers/Tasks/Settings cycles
without an update. Browser private memory grew only 0.4 MiB, the storage
process grew 0.1 MiB, storage usage remained zero, DOM nodes remained exactly
448, and every growth gate passed. This isolated an earlier connection timeout
to the rolling API handoff rather than steady-state navigation. Release
`0.1.0-4c2b31542f31` extends the terminal attachment retry schedule to the same
bounded 15.85-second recovery window used by authenticated application
bootstrap. A deliberate API-only restart during an active three-minute Edge
run then produced five expected gateway errors, recovered the authenticated
runtime and selected terminal in 7.1 seconds, completed eight navigation
cycles, and passed all browser, storage-process, owned-process, and renderer
memory bounds. Terminal-host PID `400662` and all three worker sessions were
preserved.

The 2026-08-15 current-build checkpoint ran release
`0.1.0-dev-b278aa2531e1` for 600 seconds in isolated headless Edge while the
complete dogfood navigation loop cycled nine times. Browser PID `17704` stayed
fixed for 59 samples. Post-warmup browser private growth was 3.4 MiB at a 0.16
MiB/minute fitted slope; the storage service grew 0.18 MiB at 0.03 MiB/minute;
the complete owned tree grew 29.9 MiB at 2.59 MiB/minute; and renderer heap grew
2.1 MiB at 0.05 MiB/minute. Every material-growth and slope gate passed,
browser storage remained zero bytes, DOM nodes stayed between 1,022 and 1,059,
and no authenticated-page error occurred. The harness closed Edge after writing
its verdict. This revalidates the heavily expanded Apiary, Jira, and email UI on
the current build; it remains a bounded checkpoint rather than the required
24-hour promotion soak.

The later exact-release checkpoint on `0.1.0-dev-43e82f295732` first exposed a
Windows harness race: one normal short-lived Chromium helper exited between
CDP enumeration and `Get-Process`, causing PowerShell to return failure after
three otherwise healthy samples. The sampler now ignores only vanished helper
IDs while still pinning the actual browser PID, with a cross-platform
regression test. The corrected 600-second run pinned Edge PID `61700` for 59
samples and completed nine full navigation cycles. Post-warmup browser private
growth was 3.6 MiB at 0.31 MiB/minute; the storage service grew 0.16 MiB at
0.02 MiB/minute; the complete owned tree grew 20.2 MiB at 2.09 MiB/minute; and
renderer heap grew 1.55 MiB at 0.04 MiB/minute. Every growth and slope gate
passed, browser storage remained zero bytes, DOM nodes stayed between 1,020 and
1,059, no authenticated-page error occurred, and the isolated Edge process
closed after the verdict.

## Read-only live observation

`scripts/dogfood/observe-live-soak.sh` monitors the actual dogfood crew without
changing it. It snapshots every session that is running when observation
begins, proves each remains running, pins the terminal-host PID, and samples
service memory, task counts, retained history, and dropped history. It makes no
POST, PUT, PATCH, or DELETE request and never restarts a service. The API PID is
pinned as well, so restarting either owned process fails the run instead of
making a growing memory series appear healthy by resetting it midway.

This is the safe choice while the operator is doing real work. It complements
the synthetic harness: live observation proves that normal use remains bounded,
while the synthetic run remains responsible for deliberate API replacement and
worker cleanup behavior.

The first bounded validation on 2026-08-13 observed three existing sessions for
six samples over 60 seconds. Terminal-host PID `400662` remained unchanged, API
memory stayed between 4.4 and 4.7 MiB, the host cgroup stayed between 882.8 and
883.6 MiB, retained history remained exactly 2,466,880 bytes, and dropped
history remained zero. This validates the read-only harness; it is not the
24-hour promotion result.

A later uninterrupted validation observed the same three live sessions for 20
samples over 600 seconds. API PID `959445` stayed fixed with memory between
6.7 and 7.5 MiB; terminal-host PID `400662` stayed fixed with its complete
cgroup between 1.19 and 1.21 GiB. Retained history grew only with real terminal
activity from 69,493,127 to 69,504,137 bytes, and dropped history remained zero.
The run used API release `0.1.0-cad55ededfc5` and terminal-host `0.1.0`.

The next uninterrupted validation extended that evidence to 1,800 seconds and
60 samples with the same three live sessions. API PID `969807` stayed fixed at
5.5 to 7.2 MiB. Terminal-host PID `400662` stayed fixed while its complete
cgroup, including the three Claude processes, remained between 1.21 and 1.24
GiB. Retained history ranged from 69,519,004 to 69,526,930 bytes with zero
dropped bytes. The run passed on API release `0.1.0-da686ff83eb1`; it remains a
bounded overnight checkpoint rather than the required 24-hour promotion soak.

The 2026-08-13 exact-release run for `0.1.0-0c4ecb179c10` completed 120
read-only samples over one hour with three live sessions, API PID `1037954`,
and terminal-host PID `400662` unchanged. API RSS stayed between 5,914,624 and
8,318,976 bytes; the terminal-host cgroup stayed between 1,658,208,256 and
1,776,128,000 bytes; retained history advanced only 12,212 bytes and dropped
history remained zero. The paired Edge run also passed all growth and slope
limits over 120 samples and 59 active navigation cycles.

The 2026-08-15 exact-release run for `0.1.0-dev-371eb6297ac6` observed the
current real dogfood session for 20 read-only samples over 600 seconds. API PID
`3110299` remained fixed with memory between 31,895,552 and 33,312,768 bytes;
terminal-host PID `2966127` remained fixed with its complete cgroup between
358,846,464 and 359,657,472 bytes. Retained terminal history stayed exactly
172,922,017 bytes and dropped history remained zero. No worker, service,
terminal input, Jira record, or browser state was changed by the observation.
