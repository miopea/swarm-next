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
samples. No application deployment was needed. Results remain pending; normal
workload and service/session continuity must be checked before interpretation.

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
