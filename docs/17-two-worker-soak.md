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
