#!/bin/sh
# Two real Hives on this machine — a Keeper and a member — joined over HTTP
# through a proxy that answers 502 while the Keeper is down, then driven through
# the Apiary features that only exist between two Hives: the member's version
# report, recovery after the Keeper restarts, watching, and takeover.
#
# Usage: two-hive-acceptance.sh [BUNDLE_DIR]
#   BUNDLE_DIR  an unpacked release bundle (bin/, web/). Omitted: this checkout's
#               target/release binaries and web/dist, built first if missing.
#
# Everything is disposable: state lives under /tmp/swarm-two-hive.*, the
# processes are transient user units, and all of it is removed on exit. What a
# failure leaves behind to look at goes to target/two-hive/ instead.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
bundle=${1:-}
test_root=$(mktemp -d /tmp/swarm-two-hive.XXXXXX)
chmod 700 "$test_root"
units="swarm-two-hive-keeper swarm-two-hive-member swarm-two-hive-engine-keeper swarm-two-hive-engine-member swarm-two-hive-proxy"

cleanup() {
  for unit in $units; do systemctl --user stop "$unit" >/dev/null 2>&1 || true; done
  for unit in $units; do systemctl --user reset-failed "$unit" >/dev/null 2>&1 || true; done
  # TWO_HIVE_KEEP=1 leaves both Hives' databases behind to read afterwards.
  if [ "${TWO_HIVE_KEEP:-0}" = 1 ]; then
    echo "kept: $test_root" >&2
  else
    case "$test_root" in /tmp/swarm-two-hive.*) rm -rf -- "$test_root" ;; esac
  fi
}
trap cleanup EXIT HUP INT TERM

if [ -z "$bundle" ]; then
  bundle="$test_root/bundle"
  mkdir -p "$bundle/bin"
  # Always built: testing binaries older than the checkout is how a green run
  # can describe code nobody is shipping.
  (cd "$repo_root" && PATH="$HOME/.cargo/bin:$PATH" CARGO_INCREMENTAL=0 cargo build --release --quiet -p swarm-api -p swarm-terminal-host)
  (cd "$repo_root/web" && pnpm build >/dev/null)
  for binary in swarm-api swarm-terminal-host swarmctl; do
    [ -x "$repo_root/target/release/$binary" ] && ln -s "$repo_root/target/release/$binary" "$bundle/bin/$binary"
  done
  ln -s "$repo_root/web/dist" "$bundle/web"
fi
test -x "$bundle/bin/swarm-api"
test -x "$bundle/bin/swarm-terminal-host"
test -f "$bundle/web/index.html"

keeper_port=8881
member_port=8882
proxy_port=8883
token=two-hive-acceptance-only
stub_dir="$repo_root/scripts/dogfood/two-hive"

for unit in $units; do systemctl --user reset-failed "$unit" >/dev/null 2>&1 || true; done

start_side() {
  side=$1 port=$2 public=$3
  side_root="$test_root/$side"
  mkdir -p "$side_root/workspaces/queen" "$side_root/history"
  systemd-run --user --quiet --unit="swarm-two-hive-engine-$side" \
    --property=RuntimeMaxSec=1h --property=MemoryMax=256M \
    --property=WorkingDirectory="$side_root" \
    --setenv=PATH="$stub_dir:/usr/bin:/bin" \
    --setenv=SWARM_WORKSPACE_ROOTS="$side_root/workspaces" \
    --setenv=SWARM_TERMINAL_HISTORY_DIR="$side_root/history" \
    --setenv=SWARM_TERMINAL_SOCKET="$side_root/engine.sock" \
    --setenv=RUST_LOG="swarm_terminal_host=debug,swarm_terminal=debug" \
    "$bundle/bin/swarm-terminal-host"
  # Written out so the driver can bring this side back after stopping it: a
  # transient unit is gone once stopped and cannot simply be started again.
  cat > "$side_root/start-api.sh" <<START
#!/bin/sh
exec systemd-run --user --quiet --unit="swarm-two-hive-$side" \\
  --property=RuntimeMaxSec=1h --property=MemoryMax=512M \\
  --property=WorkingDirectory="$side_root" \\
  --setenv=PATH="$stub_dir:/usr/bin:/bin" \\
  --setenv=SWARM_API_BIND="127.0.0.1:$port" \\
  --setenv=SWARM_PUBLIC_BASE_URL="$public" \\
  --setenv=SWARM_WEB_ROOT="$bundle/web" \\
  --setenv=SWARM_DATABASE_PATH="$side_root/swarm.sqlite3" \\
  --setenv=SWARM_OPERATOR_CONFIG_PATH="$side_root/operator.json" \\
  --setenv=SWARM_LEGACY_DATABASE_PATH="$side_root/no-legacy.sqlite3" \\
  --setenv=SWARM_WORKSPACE_ROOTS="$side_root/workspaces" \\
  --setenv=SWARM_AGENT_CONFIG_ROOT="$side_root/agents" \\
  --setenv=SWARM_TERMINAL_SOCKET="$side_root/engine.sock" \\
  --setenv=SWARM_OPERATOR_TOKEN="$token" \\
  --setenv=RUST_LOG="swarm_api=debug,swarm_terminal=debug" \\
  "$bundle/bin/swarm-api"
START
  sh "$side_root/start-api.sh"
}

# The member reaches the Keeper only through the proxy, as a field member
# reaches it through a tunnel.
systemd-run --user --quiet --unit=swarm-two-hive-proxy \
  --property=RuntimeMaxSec=1h \
  "$(command -v node)" "$stub_dir/proxy.cjs" "$proxy_port" "$keeper_port" "$test_root/proxy-fault"
start_side keeper "$keeper_port" "http://localhost:$proxy_port"
start_side member "$member_port" "http://localhost:$member_port"

started_at=$(date '+%Y-%m-%d %H:%M:%S')
artifacts="$repo_root/target/two-hive"
rm -rf "$artifacts"
KEEPER="http://127.0.0.1:$keeper_port" MEMBER="http://127.0.0.1:$member_port" \
  TOKEN="$token" KEEPER_UNIT=swarm-two-hive-keeper KEEPER_START="$test_root/keeper/start-api.sh" \
  PROXY_FAULT="$test_root/proxy-fault" ARTIFACTS="$artifacts" \
  node "$repo_root/scripts/dogfood/two-hive/driver.cjs" && passed=0 || passed=1

# Both Hives' own accounts of the run, kept whether it passed or not. Asking
# someone to reproduce a failure by hand, to find out what each side thought
# happened, is the leg work this run exists to remove — and a pass can still
# hide a side that was misbehaving the whole time.
mkdir -p "$artifacts"
for side in keeper member; do
  journalctl --user --no-pager -o short-iso --since "$started_at" \
    -u "swarm-two-hive-$side" -u "swarm-two-hive-engine-$side" > "$artifacts/$side.log" 2>&1 || true
done
echo "logs: $artifacts/keeper.log $artifacts/member.log" >&2

# ⚠️ A PASS CAN HIDE A LOOP. The member and Keeper once set each other off
# about fifty times a second — every check above still passed, and only the
# member's own log showed it. A run this long needs a few dozen passes at most:
# the 15-second pacing, plus one per watch, takeover and restart announcement.
passes=$(grep -c "federation pass starting" "$artifacts/member.log" || true)
if [ "$passes" -gt 150 ]; then
  echo "FAIL  the member ran $passes synchronization passes in one run; something is starting them in a loop" >&2
  passed=1
else
  echo "PASS  the member ran $passes synchronization passes, not a loop" >&2
fi
exit "$passed"
