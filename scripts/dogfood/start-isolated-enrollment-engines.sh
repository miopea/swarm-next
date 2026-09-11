#!/bin/sh
# Sign-in checks require a host handshake. These hosts cannot reach providers.
set -eu
bundle=${1:?Pass the installed Swarm bundle}
test_root=${2:?Pass the state directory printed by start-isolated-enrollment-pair.sh}
case "$test_root" in /tmp/swarm-enrollment-pair.*) ;; *) exit 1;; esac
test -x "$bundle/bin/swarm-terminal-host"
for side in keeper member; do
  side_root="$test_root/$side"
  test -d "$side_root/workspaces"
  systemd-run --user --quiet --unit="swarm-enrollment-engine-$side" \
    --property=RuntimeMaxSec=2h --property=MemoryMax=256M \
    --property=PrivateNetwork=yes \
    --property=WorkingDirectory="$side_root" \
    --setenv=PATH=/usr/bin:/bin \
    --setenv=SWARM_WORKSPACE_ROOTS="$side_root/workspaces" \
    --setenv=SWARM_TERMINAL_HISTORY_DIR="$side_root/history" \
    --setenv=SWARM_TERMINAL_SOCKET="$side_root/no-worker-engine.sock" \
    "$bundle/bin/swarm-terminal-host"
done
