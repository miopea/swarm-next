# September 8 live resource and restore checkpoint

Runtime: `1.6.0-dev-940d3c8ed1f0-20260908230927-1746491`.
API PID 1748360; terminal-host PID 1547164. No engine restart or release.

## Completed resource sample

The observer finished successfully: 20 samples over its configured 600-second
window, 574 seconds between first and last samples. All 16 original sessions
and both service process identities remained. Final CPU averages / largest
sample intervals, expressed as percent of one core:

| Layer | Average | Largest interval |
| --- | ---: | ---: |
| API cgroup | 4.81% | 20.42% |
| Engine process only | 3.24% | 5.68% |
| Engine cgroup including workers | 192.67% | 367.59% |

API memory ranged 38,125,568–120,418,304 bytes; worker-inclusive memory ranged
10,985,586,688–16,058,560,512 bytes. History retention ranged
534,812,530–536,870,377 bytes with zero reported dropped history bytes. Collection
took 0–1 seconds per sample. This verifies short-window continuity and CPU
attribution, not steady-state memory stability or a multi-day soak. A longer
equivalent workload must distinguish warming/caching from sustained growth.

### Observer identity and preliminary checkpoint

The bounded read-only 600-second sampler was PID 1749352, log
`/tmp/swarm-live-soak-940d3c8e.log`, samples
`/home/bschleifer/.local/state/swarm/soak/20260908T231159Z-live-samples.csv`.
It checks original session/service identity, samples every 30 seconds and
retains no terminal content. Do not deploy during this observation window.

Seven preliminary samples span 181 seconds. API cgroup CPU averaged 3.76%
of one core, engine process alone 2.63%, and the terminal-host cgroup including
worker processes 174.38%. All 16 sessions remained. API memory ranged from
38,125,568 to 69,201,920 bytes; the worker-inclusive group ranged from
10,985,586,688 to 14,377,746,432 bytes. These are partial observations, not
acceptance or evidence of a leak. Do not attribute worker-inclusive load to
the engine process itself.

One local-loopback read measured coordinator payload 179,565 bytes / 54.8 ms
and tasks 170,242 bytes / 10.0 ms. This does not measure remote network,
JSON parsing, rendering or latency percentiles. No speculative payload or
polling optimization follows from a single request measurement.

## Real Edge demo reload

### Follow-up memory attribution

After the short sample, `/proc` showed API anonymous RSS 112,696 KiB and
file-backed RSS 26,384 KiB, with no swap. Cgroup anonymous charge was
115,400,704 bytes. Thus the increase cannot be dismissed as file-cache accounting;
this still does not establish a leak.

The read-only sampler now records process RSS, anonymous RSS and file-backed RSS
independently from cgroup totals. Missing fields refuse the sample; old CSVs
remain readable with null process-memory attribution, not invented zeros. Six
summary tests and Bash syntax checks pass. The first live extended row populated
all three fields successfully.

A bounded 1,800-second follow-up started under PID 1768185 in
`/tmp/swarm-memory-attribution.nlYZpA`, log `observer.log`, using 30-second
samples. No application deployment was needed.

The observer ended without a completion report. Preserve its 59 samples as a
partial run, not completed 30-minute acceptance: first-to-last span is 1,755
seconds, last sample elapsed 1,756 seconds. At 23:57 UTC the original observer
process was gone; a subsequent API census showed 12 running sessions instead of
16, while API PID 1748360 and engine PID 1547164 were unchanged. This is consistent
with the original-session continuity guard refusing the next sample, but the
old script's empty error log cannot establish the exact failing check or why
sessions ended. No controller restart or deployment occurred in this window.

Across the partial series, API anonymous RSS rose from 115,400,704 to 341,786,624
bytes (110 to 326 MiB), and RSS from 140,439,552 to 367,542,272 bytes. File-backed
RSS stayed between 25,038,848 and 25,837,568 bytes. A separate `/proc` read found
nine threads, no swap and most anonymous resident memory outside the main heap
mapping. Allocator retention versus retained application data remains unresolved;
neither a leak nor a stable plateau is proved. This does not close PERF-01/PERF-02.

Average/max interval CPU, as percent of one core: API 4.81/45.44, engine process
2.50/9.61, engine cgroup including providers 280.70/751.87. All recorded samples
showed 16 sessions, history remained bounded and no dropped history bytes were
reported. Those observations do not restore the missing final continuity proof.

The observer now reports failure phase, exit status and completed sample count,
and explicitly names a failed original-session check without terminal content.
It never logs shell commands or credentials. A Linux failure fixture proves exit
28 retains its status, reports initialization failure and removes its temporary
authentication file without exposing the fictional token. Bash syntax passes.
The gated passkey validation correctly refused the missing observation report;
after the observer was independently confirmed ended, separate validation began.

Used a separate tab at the authoritative `swarm.bfgsolutions.net`. Quick
navigation selected only the already-running **Swarm Dogfood Contract** demo.
Full page reload at `?surface=workers` restored that same worker and its
Terminal input without a manual redraw. No project worker terminal was opened
or typed into, and no demo prompt was submitted.

Rendered diagnostics recorded one completed snapshot: 67,155 bytes, 31 ms
total application, 30 ms state application and 1 ms geometry/focus. No
follow-up sizing attempt was recorded. These are elapsed application phases,
not CPU, confirmed paint, websocket connect time or total reload latency.
One observation does not close desktop jumping or mobile recovery acceptance.

Diagnostics separately captured an event with no interaction ID at 1,088 ms,
including 989 ms input delay. The slowest identified interaction was 112 ms.
The server reported no measured pressure at the sampled time. No event target
or cause was identified; an ungrouped event cannot be called a slow keystroke
or attributed to Swarm code without stronger evidence.

## API allocator retention experiment, 2026-09-09 UTC

PERF-01 follow-up on the same eight-core, 32-GiB Linux/glibc 2.39 host,
API build `fcb4f1743897`. A temporary, non-shipped preload library read only
aggregate `mallinfo2` counters: one observer thread, 41 samples at 30-second
intervals, no allocator interception, trimming, pointers or application content.
Both default and two-arena observations reached their explicit final sample.

| Allocator bytes at 20 minutes | Default | `MALLOC_ARENA_MAX=2` |
| --- | ---: | ---: |
| Total arena space | 178,614,272 | 96,219,136 |
| In use | 12,098,512 | 20,814,784 |
| Free within arenas | 166,515,760 | 75,404,352 |
| Separate mmap allocations | 0 | 0 |

The candidate ended with 46% less arena space despite more live allocations.
This identifies substantial allocator retention in this workload, not a proof
that every prior memory increase was the same cause or that memory is capped.
The service default now limits arena proliferation only in the API; it does
not change the terminal host, providers, Rust allocator, or correctness policy.
[GNU documents the arena-count setting](https://sourceware.org/glibc/manual/latest/html_node/Memory-Allocation-Tunables.html).
An operator EnvironmentFile or service drop-in can override the default;
`MALLOC_ARENA_MAX=0` restores glibc's default policy on the next API-only restart.

The restart preserved engine PID 1547164 and all 12 original session IDs.
The final strict equality guard exited 8 because a thirteenth session appeared,
not because an original session disappeared. Preserve that refused aggregate
result: this was normal live use, not a perfectly controlled identical workload.
API PID 1909140 and the engine stayed unchanged through the candidate window.
Evidence: `/tmp/swarm-arena-comparison.oRYxDn/{default,arena2}-allocator.log`.
Both temporary systemd preload configurations were removed after startup;
neither the diagnostic library nor its flags ship in the package.

Separate 80-request, sequential, read-only coordinator checks discarded bodies
and verified API, engine and session identity before and after. Default mean/max
were 34.37/71.94 ms; candidate final mean/max were 23.80/54.19 ms, with no RSS
increase during either burst. Request-window API CPU was 2.5875 seconds default
and 2.0204 seconds candidate. Reports are
`/tmp/swarm-allocator-default-requests.pl1uWY` and
`/tmp/swarm-arena2-final-requests.6HZ2Dv`. Overall background load differed, so
these narrow checks do not establish universal latency or CPU improvement.

The isolated package lifecycle suite passed, including failure/rollback paths
and a new assertion that the allocator setting exists only in the API unit.
In a separate Edge tab at the authoritative URL, six switches between the two
already-running demo workers retained terminal input. A full reload restored
the same demo conversation; its 68,612-byte snapshot applied in 32 ms, with no
follow-up sizing attempt recorded. One later browser-control navigation timed
out and succeeded on retry. These observations do not close desktop jumping,
browser sluggishness, mobile recovery, PERF-02, or long-duration acceptance.
