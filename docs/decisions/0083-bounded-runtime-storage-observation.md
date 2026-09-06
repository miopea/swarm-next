# ADR 0083: Bounded runtime storage observation

Status: accepted implementation design under the approved diagnostics maturity scope.

The September 6 development Hive reached 99% root usage while normal diagnostics
reported only memory and compute pressure. Reproducible caches were cleaned with
operator approval; automatic deletion is not authorized by this design.

The private resource endpoint samples three fixed storage roles: system root,
temporary storage, and configured Hive database storage. It reports available and
total bytes, never filesystem paths or directory contents. Roles may share a
filesystem; their capacity must not be summed. Unsupported, missing, failed or
timed-out observations are unavailable, not healthy.

Sampling reuses the existing request-driven resource refresh. At most two owned
blocking probes may exist per API process, each covering at most three paths.
The response deadline is two seconds. A timed-out probe retains its admission
permit until the filesystem call returns; repeated requests cannot accumulate
unbounded tasks. No scanning, shell commands or new background loop is introduced.

Available space below 2 GiB is advisory; below 512 MiB is critical. These explicit
byte thresholds describe short-term capacity, not predicted exhaustion time or
database integrity. Runtime shows the warning, Diagnostics names affected roles,
and recovery becomes quiet on the next successful sample. This is observation
only: no worker admission change, cleanup, restart, push or threshold-created
operator decision. A failed operation still follows its existing recovery path.

Verify threshold boundaries, missing evidence, bounded concurrent/time-out work,
content-free serialization, private endpoint access, frontend warnings and return
to quiet. Rendered acceptance remains required in the development Hive.
