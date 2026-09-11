#!/bin/sh
# Two loopback-only, disposable Hives for browser enrollment acceptance.
# No live database, credentials, worker engine, or project folders are reused.
set -eu
bundle=${1:?Pass an installed Swarm bundle directory}
test -x "$bundle/bin/swarm-api"
test -f "$bundle/web/index.html"
test_root=$(mktemp -d /tmp/swarm-enrollment-pair.XXXXXX)
chmod 700 "$test_root"
for side in keeper member; do
  case "$side" in keeper) port=8871;; member) port=8872;; esac
  side_root="$test_root/$side"
  mkdir -p "$side_root/workspaces/queen"
  systemd-run --user --quiet --unit="swarm-enrollment-test-$side" \
    --property=RuntimeMaxSec=2h --property=MemoryMax=512M \
    --property=WorkingDirectory="$side_root" \
    --setenv=SWARM_API_BIND="127.0.0.1:$port" \
    --setenv=SWARM_PUBLIC_BASE_URL="http://localhost:$port" \
    --setenv=SWARM_WEB_ROOT="$bundle/web" \
    --setenv=SWARM_DATABASE_PATH="$side_root/swarm.sqlite3" \
    --setenv=SWARM_OPERATOR_CONFIG_PATH="$side_root/operator.json" \
    --setenv=SWARM_LEGACY_DATABASE_PATH="$side_root/no-legacy.sqlite3" \
    --setenv=SWARM_WORKSPACE_ROOTS="$side_root/workspaces" \
    --setenv=SWARM_AGENT_CONFIG_ROOT="$side_root/agents" \
    --setenv=SWARM_TERMINAL_SOCKET="$side_root/no-worker-engine.sock" \
    --setenv=SWARM_OPERATOR_TOKEN=fictional-enrollment-test-only \
    "$bundle/bin/swarm-api"
done
printf 'Isolated state: %s\nKeeper: http://localhost:8871\nMember: http://localhost:8872\n' "$test_root"
printf 'Stop with: systemctl --user stop swarm-enrollment-test-keeper swarm-enrollment-test-member\n'
