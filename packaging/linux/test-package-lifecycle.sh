#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
test_root=$(mktemp -d)
trap 'case "$test_root" in /tmp/*) rm -rf -- "$test_root";; esac' EXIT HUP INT TERM

export HOME="$test_root/home"
export XDG_RUNTIME_DIR="$test_root/runtime"
export SWARM_INSTALL_ROOT="$HOME/.local/lib/swarm-next"
export SWARM_CONFIG_ROOT="$HOME/.config/swarm-next"
export SWARM_STATE_ROOT="$HOME/.local/state/swarm-next"
export SWARM_SYSTEMD_USER_ROOT="$HOME/.config/systemd/user"
export SWARM_BIN_ROOT="$HOME/.local/bin"
export SWARM_WORKSPACE_ROOT="$HOME/workspaces"
export SWARM_SYSTEMCTL_BIN="$test_root/systemctl"
export SWARM_CURL_BIN="$test_root/curl"
export SWARM_HEALTH_ATTEMPTS=1
mkdir -p "$HOME" "$XDG_RUNTIME_DIR"

cat > "$SWARM_SYSTEMCTL_BIN" <<'EOF'
#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$HOME/systemctl.log"
EOF
cat > "$SWARM_CURL_BIN" <<'EOF'
#!/bin/sh
set -eu
output=
previous=
for argument in "$@"; do
  if [ "$previous" = "--output" ]; then output=$argument; fi
  previous=$argument
done
if [ -n "$output" ]; then
  printf 'rollback-database\n' > "$output"
  exit 0
fi
version=$(cat "$SWARM_INSTALL_ROOT/current/VERSION")
printf '%s\n' "$version" >> "$HOME/curl.log"
[ "$version" != "3.0.0" ] && [ "$version" != "6.0.0" ]
EOF
chmod +x "$SWARM_SYSTEMCTL_BIN" "$SWARM_CURL_BIN"

make_bundle() {
  version=$1
  protocol=${2:-5}
  bundle="$test_root/bundle-$version"
  mkdir -p "$bundle/bin" "$bundle/web/assets" "$bundle/systemd-user"
  for binary in swarm-api swarm-terminal-host swarmctl; do
    cat > "$bundle/bin/$binary" <<'EOF'
#!/bin/sh
if [ "$(basename "$0")" = "swarmctl" ]; then
  command=${1:-}
  printf '%s\n' "$command" >> "$HOME/swarmctl.log"
  if [ "$command" = "status" ]; then
    running=0
    [ ! -f "$HOME/running-sessions" ] || running=$(cat "$HOME/running-sessions")
    printf '{"protocol_version":5,"running_sessions":%s}\n' "$running"
  fi
fi
exit 0
EOF
    chmod +x "$bundle/bin/$binary"
  done
  printf '<!doctype html><title>test</title>\n' > "$bundle/web/index.html"
  printf 'export const version = "%s";\n' "$version" > "$bundle/web/assets/app-$version.js"
  cp "$repo_root/packaging/systemd-user/"*.in "$bundle/systemd-user/"
  cp "$repo_root/packaging/linux/swarm-next-package" "$bundle/"
  chmod +x "$bundle/swarm-next-package"
  printf '%s\n' "$version" > "$bundle/VERSION"
  printf '%s\n' "$protocol" > "$bundle/PROTOCOL"
  (cd "$bundle" && find bin web systemd-user -type f -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS && sha256sum swarm-next-package >> SHA256SUMS)
}

make_bundle 1.0.0
make_bundle 2.0.0
make_bundle 3.0.0
make_bundle 4.0.0 6
make_bundle 5.0.0
make_bundle 6.0.0 7
package="$repo_root/packaging/linux/swarm-next-package"

# Initial install owns both API/browser and terminal-host pointers.
"$package" install "$test_root/bundle-1.0.0"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "1.0.0" ]
[ -f "$SWARM_CONFIG_ROOT/swarm-next.env" ]
[ "$(stat -c %a "$SWARM_CONFIG_ROOT/swarm-next.env")" = "600" ]
grep -q '127.0.0.1:8766' "$SWARM_CONFIG_ROOT/swarm-next.env"
grep -q "SWARM_WORKSPACE_ROOTS=$SWARM_WORKSPACE_ROOT" "$SWARM_CONFIG_ROOT/swarm-next.env"
grep -q "$SWARM_INSTALL_ROOT/current/bin/swarm-api" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service"
grep -q "$SWARM_INSTALL_ROOT/host-current/bin/swarm-terminal-host" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-terminal-host.service"
grep -q "$SWARM_INSTALL_ROOT/current/swarm-next-package reconcile-host-if-idle" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-host-reconcile.service"
grep -q "PathChanged=$SWARM_STATE_ROOT/worker-engine-maintenance.request" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-host-reconcile.path"
grep -q '^OnUnitActiveSec=2min$' "$SWARM_SYSTEMD_USER_ROOT/swarm-next-host-reconcile.timer"
grep -q 'swarm-next-host-reconcile.path' "$SWARM_SYSTEMD_USER_ROOT/swarm-next.target"
grep -q 'swarm-next-host-reconcile.timer' "$SWARM_SYSTEMD_USER_ROOT/swarm-next.target"
grep -q "SWARM_ASSET_ROOT=$SWARM_INSTALL_ROOT/assets" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service"
grep -q "SWARM_DATABASE_PATH=$SWARM_STATE_ROOT/swarm-next.sqlite3" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service"
grep -q "SWARM_MAINTENANCE_REQUEST_PATH=$SWARM_STATE_ROOT/worker-engine-maintenance.request" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service"
[ -f "$SWARM_INSTALL_ROOT/assets/app-1.0.0.js" ]
[ -d "$SWARM_WORKSPACE_ROOT/queen" ]
grep -q "ReadWritePaths=$SWARM_STATE_ROOT" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service"
grep -q "CLAUDE_CONFIG_DIR=$SWARM_STATE_ROOT/providers/claude" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-terminal-host.service"
grep -q 'PATH=%h/.local/bin:/usr/local/bin:/usr/bin:/bin' "$SWARM_SYSTEMD_USER_ROOT/swarm-next-terminal-host.service"
grep -q '^RuntimeDirectory=swarm-next$' "$SWARM_SYSTEMD_USER_ROOT/swarm-next-terminal-host.service"
if grep -q '^RuntimeDirectory=' "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service"; then
  echo "API must not own the terminal host runtime directory" >&2
  exit 1
fi
[ -x "$SWARM_INSTALL_ROOT/current/swarm-next-package" ]
[ -d "$SWARM_STATE_ROOT/providers/claude" ]
if command -v systemd-analyze >/dev/null 2>&1; then
  systemd-analyze --user verify \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-next-terminal-host.service" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-next-host-reconcile.service" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-next-host-reconcile.path" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-next-host-reconcile.timer" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-next.target"
fi

# Compatible API/browser updates preserve an active sidecar and its sessions.
printf '1\n' > "$HOME/running-sessions"
: > "$HOME/systemctl.log"
: > "$HOME/swarmctl.log"
"$package" update "$test_root/bundle-2.0.0"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/previous/VERSION")" = "1.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "1.0.0" ]
grep -q '^--user restart swarm-next-api.service$' "$HOME/systemctl.log"
if grep -Eq 'stop swarm-next.target|restart swarm-next-terminal-host.service' "$HOME/systemctl.log"; then
  echo "compatible update touched the terminal host" >&2
  exit 1
fi
if grep -Eq '^drain$|^wait-ready$' "$HOME/swarmctl.log"; then
  echo "compatible update drained active workers" >&2
  exit 1
fi
[ -f "$SWARM_INSTALL_ROOT/assets/app-1.0.0.js" ]
[ -f "$SWARM_INSTALL_ROOT/assets/app-2.0.0.js" ]

# API rollback is also sidecar-safe while a worker is active.
: > "$HOME/systemctl.log"
"$package" rollback
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "1.0.0" ]
grep -q '^--user restart swarm-next-api.service$' "$HOME/systemctl.log"
if grep -q 'swarm-next-terminal-host.service' "$HOME/systemctl.log"; then
  echo "API rollback touched the terminal host" >&2
  exit 1
fi
"$package" update "$test_root/bundle-2.0.0"

# Host reconciliation drains atomically and refuses to stop an active worker.
if "$package" reconcile-host; then
  echo "active host reconciliation unexpectedly succeeded" >&2
  exit 1
fi
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "1.0.0" ]
grep -q '^cancel-drain$' "$HOME/swarmctl.log"
# The timer path treats active work as a healthy deferred state and leaves new
# session admission open after its atomic drain/status check.
: > "$HOME/swarmctl.log"
"$package" reconcile-host-if-idle
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "1.0.0" ]
grep -q '^drain$' "$HOME/swarmctl.log"
grep -q '^cancel-drain$' "$HOME/swarmctl.log"
printf '0\n' > "$HOME/running-sessions"
: > "$HOME/systemctl.log"
"$package" reconcile-host
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "2.0.0" ]
grep -q '^--user restart swarm-next-terminal-host.service$' "$HOME/systemctl.log"

# Protocol changes fail closed against the independently pinned host.
if "$package" update "$test_root/bundle-4.0.0"; then
  echo "incompatible protocol update unexpectedly succeeded" >&2
  exit 1
fi
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ]

# A failed API health check restores only the previous API/browser pointer.
if "$package" update "$test_root/bundle-3.0.0"; then
  echo "unhealthy update unexpectedly succeeded" >&2
  exit 1
fi
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "2.0.0" ]
[ "$(tail -n 2 "$HOME/curl.log" | tr '\n' ' ')" = "3.0.0 2.0.0 " ]

# Explicit protocol migration refuses active workers, then atomically switches
# both processes while retaining the old API and host for rollback.
printf '1\n' > "$HOME/running-sessions"
if "$package" migrate-protocol "$test_root/bundle-4.0.0"; then
  echo "active protocol migration unexpectedly succeeded" >&2
  exit 1
fi
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "2.0.0" ]
printf '0\n' > "$HOME/running-sessions"
: > "$HOME/systemctl.log"
"$package" migrate-protocol "$test_root/bundle-4.0.0"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "4.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "4.0.0" ]

[ "$(cat "$SWARM_INSTALL_ROOT/previous/VERSION")" = "2.0.0" ]
grep -q '^--user stop swarm-next.target$' "$HOME/systemctl.log"
grep -q '^--user enable --now swarm-next.target$' "$HOME/systemctl.log"

# A failed protocol migration restores both independently pinned pointers.
if "$package" migrate-protocol "$test_root/bundle-6.0.0"; then
  echo "unhealthy protocol migration unexpectedly succeeded" >&2
  exit 1
fi
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "4.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "4.0.0" ]

# Database restore verifies input, creates a rollback snapshot, restarts only
# the API, and preserves the terminal host and repository root.
printf 'old-database\n' > "$SWARM_STATE_ROOT/swarm-next.sqlite3"
printf 'restored-database\n' > "$HOME/hive-backup.sqlite3"
: > "$HOME/systemctl.log"
"$package" restore "$HOME/hive-backup.sqlite3"
[ "$(cat "$SWARM_STATE_ROOT/swarm-next.sqlite3")" = "restored-database" ]
grep -q '^--user stop swarm-next-api.service$' "$HOME/systemctl.log"
grep -q '^--user start swarm-next-api.service$' "$HOME/systemctl.log"
if grep -q 'swarm-next-terminal-host.service' "$HOME/systemctl.log"; then
  echo "database restore touched the terminal host" >&2
  exit 1
fi

printf 'keep\n' > "$SWARM_STATE_ROOT/operator-data"
if SWARM_INSTALL_ROOT="$HOME/.local/lib/not-swarm" "$package" uninstall; then
  echo "unsafe uninstall root unexpectedly succeeded" >&2
  exit 1
fi
"$package" uninstall
[ ! -e "$SWARM_INSTALL_ROOT" ]
[ -f "$SWARM_STATE_ROOT/operator-data" ]
[ -f "$SWARM_CONFIG_ROOT/swarm-next.env" ]
printf 'package lifecycle smoke passed\n'
