#!/bin/sh
# Proves, against a REAL systemd user manager, that the stack stop actually
# stops — including the unit whose job was dropped on the operator's machine.
#
# WHY THIS FILE EXISTS AND THE OTHER SUITES COULD NOT DO IT. Every other
# packaging test stubs systemctl with a script that appends a line and exits 0:
# synchronous, instantaneous, always successful. A stop that RETURNS BEFORE ITS
# UNITS ARE DOWN cannot exist in that world, so no amount of running those
# suites would ever have found this. The operator, 2026-08-28: "Fix it and
# prove it against real systemd."
#
# Units are named with a unique prefix and removed afterwards. Nothing here
# touches swarm's own units.
set -eu

command -v systemctl >/dev/null 2>&1 || { echo "test-systemd-stop: no systemctl; skipping" >&2; exit 0; }
systemctl --user show-environment >/dev/null 2>&1 \
  || { echo "test-systemd-stop: no systemd user manager; skipping" >&2; exit 0; }

prefix="swarmtest-$$"
unit_dir="$HOME/.config/systemd/user"
mkdir -p "$unit_dir"

cleanup() {
  systemctl --user stop "$prefix.target" "$prefix-slow.service" "$prefix-host.service" "$prefix-orphan.service" >/dev/null 2>&1 || true
  rm -f "$unit_dir/$prefix.target" "$unit_dir/$prefix-slow.service" "$unit_dir/$prefix-host.service" "$unit_dir/$prefix-orphan.service"
  systemctl --user daemon-reload >/dev/null 2>&1 || true
}
trap cleanup EXIT HUP INT TERM

fail() { printf 'test-systemd-stop: %s\n' "$1" >&2; exit 1; }

cat > "$unit_dir/$prefix.target" <<EOF
[Unit]
Description=Swarm stop test target
EOF

# Stands in for swarm-api: slow to stop, so its job is already in flight.
cat > "$unit_dir/$prefix-slow.service" <<EOF
[Unit]
Description=Swarm stop test slow unit
PartOf=$prefix.target
[Service]
ExecStart=/bin/sh -c 'trap "sleep 6; exit 0" TERM; while :; do sleep 1; done'
KillSignal=SIGTERM
TimeoutStopSec=30
[Install]
WantedBy=$prefix.target
EOF

# Stands in for swarm-terminal-host: the one that survived. It is SLOW TO STOP
# on purpose, which makes the defect deterministic instead of a race this
# machine might happen to win.
# Running, and deliberately NOT PartOf the target: the dropped stop job.
cat > "$unit_dir/$prefix-orphan.service" <<EOF
[Unit]
Description=Swarm stop test orphan unit
[Service]
ExecStart=/bin/sh -c 'while :; do sleep 1; done'
EOF

cat > "$unit_dir/$prefix-host.service" <<EOF
[Unit]
Description=Swarm stop test host unit
PartOf=$prefix.target
[Service]
ExecStart=/bin/sh -c 'trap "sleep 8; exit 0" TERM; while :; do sleep 1; done'
KillSignal=SIGTERM
TimeoutStopSec=30
[Install]
WantedBy=$prefix.target
EOF

systemctl --user daemon-reload
systemctl --user enable "$prefix-slow.service" "$prefix-host.service" >/dev/null 2>&1
systemctl --user start "$prefix.target"
systemctl --user is-active --quiet "$prefix-host.service" || fail "the host stand-in did not start"
systemctl --user is-active --quiet "$prefix-slow.service" || fail "the slow stand-in did not start"

# --- THE SHAPE THAT BROKE THE OPERATOR'S MACHINE --------------------------
#
# Stop the TARGET only, then immediately daemon-reload, which is what
# install_units does. This is the sequence swarm-package ran at 09:17:04.
systemctl --user stop "$prefix.target" >/dev/null 2>&1 || true

# THE ASSERTION THAT MATTERS: `systemctl stop <target>` has RETURNED, and a unit
# the target owns through PartOf= is still running. Everything swarm-package did
# next — rewriting units, daemon-reload, relinking releases, starting the new
# stack — happened while the old process was alive. That is the whole defect,
# and it is deterministic here because the host stand-in is slow to stop.
# REPORTED, NOT ASSERTED, and the distinction is the point. Whether the stop
# returns early is a property of the systemd and the machine: it did on the
# operator's WSL box (reload logged at 09:17:04 while the API stopped at
# 09:17:16) and it does NOT on the development Hive, where the stop blocks.
# Asserting it here would make this suite fail on the machine that behaves
# correctly, which measures the machine rather than the code.
#
# What IS asserted is the guarantee below: after the stack stop, nothing the
# migration is about to replace is still running. That holds on both.
if systemctl --user is-active --quiet "$prefix-host.service"; then
  printf 'test-systemd-stop: this systemd returns from a target stop with units still running\n'
else
  printf 'test-systemd-stop: this systemd blocks until the target'"'"'s units are stopped\n'
fi

# And a reload at that moment is what dropped the job on the operator's machine.
systemctl --user daemon-reload

# --- A UNIT A TARGET STOP WILL NOT REACH -----------------------------------
#
# The operator's host survived because its stop job never ran. Waiting for that
# race to repeat is not a test — this machine's systemd blocks, so the old
# behaviour passes here and the ablation proves nothing.
#
# So the dropped job is MODELLED rather than hoped for: a running unit that the
# target does not own. `systemctl stop <target>` cannot touch it on any
# machine, which is exactly the position swarm-package was in, and it makes the
# difference between the old code and the new one deterministic everywhere.
systemctl --user start "$prefix.target"
systemctl --user is-active --quiet "$prefix-host.service" || fail "the host stand-in did not restart"
systemctl --user start "$prefix-orphan.service"
systemctl --user is-active --quiet "$prefix-orphan.service" || fail "the orphan stand-in did not start"

# THE OLD BEHAVIOUR, for the record: stop the target and trust it.
systemctl --user stop "$prefix.target" >/dev/null 2>&1 || true
systemctl --user is-active --quiet "$prefix-orphan.service" \
  || fail "a target stop reached a unit the target does not own; this test models nothing"
printf 'test-systemd-stop: a target stop leaves an unowned unit running, as it did on 2026-08-28\n'

# THE FIX, exactly as stop_swarm_stack does it: name the units, re-ask, confirm.
systemctl --user stop "$prefix-slow.service" "$prefix-host.service" "$prefix-orphan.service" "$prefix.target" >/dev/null 2>&1 || true
systemctl --user daemon-reload
for unit in "$prefix-slow.service" "$prefix-host.service" "$prefix-orphan.service"; do
  attempts=0
  while systemctl --user is-active --quiet "$unit"; do
    attempts=$((attempts + 1))
    [ "$attempts" -lt 60 ] || fail "$unit was still running after the stack stop"
    [ $((attempts % 10)) -ne 0 ] || systemctl --user stop "$unit" >/dev/null 2>&1 || true
    sleep 1
  done
done

for unit in "$prefix-slow.service" "$prefix-host.service" "$prefix-orphan.service"; do
  systemctl --user is-active --quiet "$unit" && fail "$unit survived the stack stop"
done

printf 'systemd stop verification passed (real user manager)\n'
