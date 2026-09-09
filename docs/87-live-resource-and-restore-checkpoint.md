# September 8–9 live resource and restore checkpoints

## September 9: bounded synthetic terminal-output checks

At 07:08–07:11 UTC, the existing Swarm Dogfood Contract session
`01a0846e-4385-7dd3-9867-64279705b742` was exercised in a separate Edge tab at
`https://swarm.bfgsolutions.net`. Its provider conversation remained
`c4435eee-57b0-4546-b6ef-dc182080e7b5`. Before each output write, process 2274183
was verified as Claude in the contract-fixture workspace with stdout `/dev/pts/11`.
These were output-only fixture writes, not commands submitted to the model:
a 249,856-byte burst (1,024 lines), then 1,843,200 bytes over 30 seconds in 30
bounded batches. Both end markers appeared on screen. Ctrl+L was used only in
that demo terminal to restore the provider display. No real-project PTY was
inspected or written, no worker was restarted, and the demo checkout stayed clean.

During the paced window, API PID 2358464 consumed 173 jiffies and engine PID
2271655 consumed 85 at 100 Hz: 1.73 and 0.85 CPU-seconds, respectively, or about
5.77% and 2.83% of one core averaged over 30 seconds. This includes ordinary
background activity and is not exclusive attribution to the synthetic stream.
API RSS moved from 68,680 to 68,708 KiB; engine RSS from 91,756 to 96,820 KiB.
The short window proves neither a plateau nor a leak. Both PIDs stayed unchanged;
App/API remained healthy on `1.6.0-dev-202519adabe9-20260909065218-2355110`.

The browser's cumulative observations, including setup/navigation and both
streams, showed 238 terminal-apply samples (mean 1 ms, max 36 ms), one 62 ms
main-thread block, and one 811 ms connection (565 ms access, 133 ms socket,
113 ms initial state). Quick navigation could be opened and searched during
the stream. This is qualitative menu usability, not provider-input latency.
The same aggregate had 73 event entries (mean 2,216 ms, max 4,088 ms) and five
navigation frame estimates (mean 835 ms, max 2,017 ms). Those slow values remain
unresolved; low terminal-apply timings do not establish smooth screen paint.
Code inspection confirms the route estimate uses two animation frames without
a timeout fallback and excludes observed hidden intervals. Visible state does
not prove the locked/remote desktop was foreground or continuously presenting.

An on-demand Chromium heap estimate was 39.4 MiB used / 58.4 MiB allocated,
without a paired baseline; it is not total browser memory or leak evidence.
One browser automation read timed out during Settings navigation and subsequent
reads succeeded. The workload is synthetic engine-to-browser output, not a
matched fifteen-worker aged-session comparison. CPU, responsiveness under normal
interactive work, and instrumentation overhead acceptance remain open.

## September 9: uninterrupted one-hour post-return observation

Both services ran `1.6.0-dev-cf83c980bf17-20260909041327-2251164`:
API PID 2252714, engine PID 2271655. Observer PID 2274770 completed its configured
3,600-second window with 120 samples, 3,592 seconds between first and last.
All twelve original sessions remained running and both service identities stayed
unchanged. The final report is in `/tmp/swarm-cf83-return-soak.kIt7SR/observer.log`;
the CSV is `20260909T043116Z-live-samples.csv` in the same directory.

| Layer | Average CPU, one core | Largest sample interval |
| --- | ---: | ---: |
| API cgroup | 2.08% | 38.36% |
| Engine process only | 1.12% | 5.17% |
| Engine cgroup including workers | 15.36% | 27.27% |

API RSS ranged from 56,418,304 to 70,017,024 bytes (53.80–66.77 MiB).
The final twenty samples, spanning the last ten minutes, all reported exactly
70,017,024 bytes. Anonymous RSS ranged 31,637,504–44,613,632 bytes. API cgroup
charge ranged 37,769,216–53,030,912 bytes; it is not the same measurement as RSS.
Worker-inclusive cgroup memory ranged 3,403,526,144–3,697,995,776 bytes.
Retained history grew from 533,040,463 to 534,807,755 bytes, with zero reported
dropped bytes. Collection took 0–1 seconds per sample.

This establishes continuity and a late-window API memory plateau under this
specific workload: twelve loaded sessions, Queen reviews and the two-demo
dependency journey in `91-engine-return-dependency-acceptance.md`. Most workers
were idle. Compilation waited until this observer exited. No deployment or
worker restart interrupted the series. It does not establish heavy multi-worker
throughput, browser/PWA performance, or a multi-day plateau. Do not compare its
CPU directly with the earlier sixteen-worker active-work sample as a speedup.

## September 8 observations (historical)

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

### Post-deployment observation interrupted, September 8, 21:25 Eastern

The one-hour observation of deployed `1bbc4cbb` did not complete. Its existing
observer terminated with `phase=session_continuity exit=22 completed_samples=35`
after an HTTP 503. Last sample: 2026-09-09T01:25:08Z, elapsed 1,028 seconds,
API RSS 112,889,856 bytes, anonymous RSS 85,442,560 bytes, 13 running sessions
and all 13 original sessions retained at that sample. Evidence remains in
`/tmp/swarm-arena-deployed-soak.H4jTtS`; do not report a one-hour soak pass.

Systemd records `swarm-host-reconcile.service` starting at 21:25:35 Eastern,
then graceful engine replacement at 21:25:36. Engine PID changed from 1547164
to 1996041, while API PID remained 1982503. A subsequent authenticated metadata
check found engine build `d1780215d3b8b0a335e9379a31dbec3f40be971dfb5778325e68205f57a9310b`,
13 running sessions, zero unreadable sessions, and entirely new session IDs.
Worker return is observed; correct provider-conversation resumption is not
verified by these metadata checks. No real worker transcript was inspected.

This inspection did not request engine maintenance. The service name alone does
not distinguish an operator request from timer reconciliation. Operator
confirmation was subsequently received: the operator manually applied the update.
This was deliberate maintenance, not an unexplained automatic engine replacement.
The interrupted observation must not be silently restarted or combined across
engine replacement into a continuity or equivalent-workload acceptance result.

### Browser navigation on deployed 4736324e

A separate authenticated Edge tab exercised three cycles through Settings,
Needs You and Queues (nine transitions). Each destination heading became
visible without a timeout. Over 55.44 seconds, DevTools thread-tick metrics
reported 0.548 seconds main-thread CPU, 0.176 seconds script execution and
0.051 seconds layout. Heap fell from 27,616,012 to 22,638,712 bytes. A subsequent
19.18-second window used 0.088 seconds main-thread CPU; listener count fell
from 868 to 635. No warning/error entries were returned by the browser log read.
These short samples do not close aged-session performance. Automation round trips
included tool overhead and are not application interaction latency measurements.

The same tab was temporarily emulated at 390 by 844 CSS pixels. Document and body
scroll widths both measured 390; visible primary navigation and header buttons
were 44 pixels high, owner links at least 48, and the visible blocker disclosure
48. The rendered queue remained readable without horizontal clipping. Desktop
viewport was restored (1465 by 1339), and performance collection was disabled.
This is responsive-layout evidence, not real Android/iOS keyboard, attachment or
terminal handoff acceptance. No project terminal was opened or changed.

### Post-VPN observation, September 9 at 03:10 UTC

The new observer (PID 2169502) completed 30 samples before its strict original-
session guard stopped it. Evidence is retained in
`/tmp/swarm-review-deployed-soak.ghb2Ar/20260909T031017Z-live-samples.csv` and
`observer.log`. At the final sample (03:24:55 UTC, elapsed 878 seconds), all 13
original sessions remained. D365 subsequently became sleeping; the operator
explicitly confirmed putting it to sleep. This is intentional workload change,
not an unexplained worker failure or an engine restart. Do not join a replacement
window to this one or report the one-hour continuity gate passed.

Direct checks after the observation retained API PID 2036888 and engine PID
1996041. API process RSS was 112,271,360 bytes at the first sample and 115,884,032
at the last; anonymous RSS was 85,909,504 and 89,325,568 respectively. These are
short-window measurements under changing real work, not long-duration acceptance.

The approved central integration configuration now includes the exclusive
`swarm` source binding while preserving the existing four app bindings and
credential. This required no service restart. Admin's single fictional request
hit its own feedback-conversation ID validation before Swarm dispatch; Admin
owns that correction and the same-request retry. End-to-end completion and an
unsent Admin reply remain unverified. Normal operator assignment to an existing
isolated demo worker can preserve the immutable source ticket while allowing
truthful worker evidence; operator completion alone cannot invent that evidence.
