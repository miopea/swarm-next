#!/bin/sh
set -eu

bundle=${1:?usage: test-release-runtime.sh RELEASE_DIR}
test_root=$(mktemp -d)
host_pid=
api_pid=
cleanup() {
  [ -z "$api_pid" ] || kill -TERM "$api_pid" 2>/dev/null || true
  [ -z "$host_pid" ] || kill -TERM "$host_pid" 2>/dev/null || true
  [ -z "$api_pid" ] || wait "$api_pid" 2>/dev/null || true
  [ -z "$host_pid" ] || wait "$host_pid" 2>/dev/null || true
  case "$test_root" in /tmp/*) rm -rf -- "$test_root";; esac
}
trap cleanup EXIT HUP INT TERM
[ -x "$bundle/swarm-next-package" ]

socket="$test_root/runtime/terminal.sock"
history="$test_root/state/history"
workspace="$test_root/workspace"
mkdir -p "$(dirname "$socket")" "$history" "$workspace"

SWARM_TERMINAL_SOCKET="$socket" \
SWARM_TERMINAL_HISTORY_DIR="$history" \
SWARM_WORKSPACE_ROOTS="$workspace" \
  "$bundle/bin/swarm-terminal-host" >"$test_root/host.log" 2>&1 &
host_pid=$!

attempts=0
while [ ! -S "$socket" ] && [ "$attempts" -lt 50 ]; do
  attempts=$((attempts + 1))
  sleep 0.1
done
[ -S "$socket" ] || { cat "$test_root/host.log" >&2; exit 1; }

SWARM_TERMINAL_SOCKET="$socket" \
SWARM_OPERATOR_TOKEN=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
SWARM_API_BIND=127.0.0.1:18766 \
SWARM_WEB_ROOT="$bundle/web" \
  "$bundle/bin/swarm-api" >"$test_root/api.log" 2>&1 &
api_pid=$!

attempts=0
until curl --fail --silent --show-error --max-time 1 http://127.0.0.1:18766/health > "$test_root/health.json"; do
  attempts=$((attempts + 1))
  [ "$attempts" -lt 50 ] || { cat "$test_root/api.log" >&2; exit 1; }
  sleep 0.1
done
grep -q '"status":"ok"' "$test_root/health.json"
curl --fail --silent --show-error http://127.0.0.1:18766/ | grep -q '<div id="root"></div>'
[ "$(curl --silent --output /dev/null --write-out '%{http_code}' http://127.0.0.1:18766/api/v1/not-a-route)" = "404" ]

kill -TERM "$api_pid" "$host_pid"
wait "$api_pid"
api_pid=
wait "$host_pid"
host_pid=
[ ! -e "$socket" ]
printf 'release runtime smoke passed\n'
