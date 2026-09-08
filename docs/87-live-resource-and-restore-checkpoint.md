# September 8 live resource and restore checkpoint

Runtime: `1.6.0-dev-940d3c8ed1f0-20260908230927-1746491`.
API PID 1748360; terminal-host PID 1547164. No engine restart or release.

## Resource sample in progress

The bounded read-only 600-second sampler is PID 1749352, log
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
