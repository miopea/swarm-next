#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
test_root=$(mktemp -d)
trap 'case "$test_root" in /tmp/*) rm -rf -- "$test_root";; esac' EXIT HUP INT TERM

export HOME="$test_root/home"
export XDG_RUNTIME_DIR="$test_root/runtime"
export SWARM_INSTALL_ROOT="$HOME/.local/lib/swarm"
export SWARM_CONFIG_ROOT="$HOME/.config/swarm"
export SWARM_STATE_ROOT="$HOME/.local/state/swarm"
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
  cp "$SWARM_STATE_ROOT/swarm.sqlite3" "$output"
  exit 0
fi
version=$(cat "$SWARM_INSTALL_ROOT/current/VERSION")
printf '%s\n' "$version" >> "$HOME/curl.log"
[ "$version" != "3.0.0" ] || printf 'database-v3\n' > "$SWARM_STATE_ROOT/swarm.sqlite3"
[ "$version" != "6.0.0" ] || printf 'database-v6\n' > "$SWARM_STATE_ROOT/swarm.sqlite3"
[ "$version" != "3.0.0" ] && [ "$version" != "6.0.0" ]
EOF
chmod +x "$SWARM_SYSTEMCTL_BIN" "$SWARM_CURL_BIN"

# Any unit that runs swarm-package must be able to write everywhere
# swarm-package writes. systemd does not fail on a path outside ReadWritePaths;
# the unit starts and then cannot write, which reads as a hang rather than a
# permission error. This has now happened twice — the release install unit and
# the worker engine reconcile — so it is checked rather than remembered.
for unit in "$repo_root"/packaging/systemd-user/*.service.in; do
  grep -q 'ExecStart=.*swarm-package' "$unit" || continue
  for required in @INSTALL_ROOT@ @UNIT_ROOT@ @STATE_ROOT@ @BIN_ROOT@; do
    grep -q "ReadWritePaths=.*$required" "$unit" || {
      printf 'unit %s runs swarm-package but cannot write %s\n' "$(basename "$unit")" "$required" >&2
      exit 1
    }
  done
done

# And every placeholder any template uses must be one the renderer substitutes.
for placeholder in $(grep -ho '@[A-Z_]*@' "$repo_root"/packaging/systemd-user/*.in | sort -u); do
  grep -q "s|$placeholder|" "$repo_root/packaging/linux/swarm-package" || {
    printf 'template placeholder %s has no substitution in swarm-package\n' "$placeholder" >&2
    exit 1
  }
done

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
  if [ "$command" = "verify-database" ]; then
    cat "$(dirname "$0")/../VERSION" >> "$HOME/verify-release.log"
  fi
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
  cp "$repo_root/packaging/linux/swarm-package" "$bundle/"
  chmod +x "$bundle/swarm-package"
  printf '%s\n' "$version" > "$bundle/VERSION"
  printf '%s\n' "$protocol" > "$bundle/PROTOCOL"
  (cd "$bundle" && find bin web systemd-user -type f -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS && sha256sum swarm-package >> SHA256SUMS)
}

make_bundle 1.0.0
make_bundle 2.0.0
make_bundle 3.0.0
make_bundle 4.0.0 6
make_bundle 5.0.0
make_bundle 6.0.0 7
package="$repo_root/packaging/linux/swarm-package"

# Initial install owns both API/browser and terminal-host pointers.
"$package" install "$test_root/bundle-1.0.0"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "1.0.0" ]
[ "$(readlink "$SWARM_BIN_ROOT/swarm-terminal-host")" = "$SWARM_INSTALL_ROOT/host-current/bin/swarm-terminal-host" ]
[ -f "$SWARM_CONFIG_ROOT/swarm.env" ]
[ "$(stat -c %a "$SWARM_CONFIG_ROOT/swarm.env")" = "600" ]
grep -q '127.0.0.1:8766' "$SWARM_CONFIG_ROOT/swarm.env"
grep -q "SWARM_WORKSPACE_ROOTS=$SWARM_WORKSPACE_ROOT" "$SWARM_CONFIG_ROOT/swarm.env"
grep -q "$SWARM_INSTALL_ROOT/current/bin/swarm-api" "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
grep -q "$SWARM_INSTALL_ROOT/host-current/bin/swarm-terminal-host" "$SWARM_SYSTEMD_USER_ROOT/swarm-terminal-host.service"
grep -q "$SWARM_INSTALL_ROOT/current/swarm-package reconcile-host-requested" "$SWARM_SYSTEMD_USER_ROOT/swarm-host-reconcile.service"
grep -q "ReadWritePaths=$SWARM_STATE_ROOT" "$SWARM_SYSTEMD_USER_ROOT/swarm-host-reconcile.service"
grep -q "PathChanged=$SWARM_STATE_ROOT/worker-engine-maintenance.request" "$SWARM_SYSTEMD_USER_ROOT/swarm-host-reconcile.path"
tr -d '\r' < "$SWARM_SYSTEMD_USER_ROOT/swarm-host-reconcile.timer" | grep -q '^OnUnitActiveSec=2min$'
grep -q 'swarm-host-reconcile.path' "$SWARM_SYSTEMD_USER_ROOT/swarm.target"
grep -q 'swarm-host-reconcile.timer' "$SWARM_SYSTEMD_USER_ROOT/swarm.target"
grep -q "SWARM_ASSET_ROOT=$SWARM_INSTALL_ROOT/assets" "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
grep -q "SWARM_DATABASE_PATH=$SWARM_STATE_ROOT/swarm.sqlite3" "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
grep -q "SWARM_MAINTENANCE_REQUEST_PATH=$SWARM_STATE_ROOT/worker-engine-maintenance.request" "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
grep -q "EnvironmentFile=-$SWARM_CONFIG_ROOT/swarm-dev.env" "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
if grep -q 'CLAUDE_CONFIG_DIR' "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"; then
  echo "Workers must use the default Claude configuration directory" >&2
  exit 1
fi
grep -q 'ReadWritePaths=%h/.claude$' "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
grep -q 'ReadWritePaths=%h/.claude.json$' "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
grep -q 'PATH=%h/.local/bin:/usr/local/bin:/usr/bin:/bin' "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
grep -q '^Wants=swarm-terminal-host.service$' "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
if grep -q '^Requires=swarm-terminal-host.service$' "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"; then
  echo "API must remain online during a controlled terminal-host restart" >&2
  exit 1
fi
grep -q "PathExists=$SWARM_STATE_ROOT/development-reload.request" "$SWARM_SYSTEMD_USER_ROOT/swarm-development-reload.path"
grep -q "ReadWritePaths=$SWARM_INSTALL_ROOT $SWARM_STATE_ROOT $SWARM_WORKSPACE_ROOT" "$SWARM_SYSTEMD_USER_ROOT/swarm-development-reload.service"
grep -q '^Environment=PATH=%h/.cargo/bin:%h/.local/share/pnpm:%h/.local/bin:/usr/local/bin:/usr/bin:/bin$' "$SWARM_SYSTEMD_USER_ROOT/swarm-development-reload.service"
[ -f "$SWARM_INSTALL_ROOT/assets/app-1.0.0.js" ]
[ -d "$SWARM_WORKSPACE_ROOT/queen" ]
grep -q "ReadWritePaths=$SWARM_STATE_ROOT" "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
if grep -q 'CLAUDE_CONFIG_DIR' "$SWARM_SYSTEMD_USER_ROOT/swarm-terminal-host.service"; then
  echo "Workers must use the default Claude configuration directory" >&2
  exit 1
fi
grep -q 'ReadWritePaths=%h/.claude$' "$SWARM_SYSTEMD_USER_ROOT/swarm-terminal-host.service"
grep -q 'ReadWritePaths=%h/.claude.json$' "$SWARM_SYSTEMD_USER_ROOT/swarm-terminal-host.service"
grep -q 'SWARM_CLAUDE_SETTINGS_PATH=%h/.claude/settings.json' "$SWARM_SYSTEMD_USER_ROOT/swarm-terminal-host.service"
grep -q 'PATH=%h/.local/bin:/usr/local/bin:/usr/bin:/bin' "$SWARM_SYSTEMD_USER_ROOT/swarm-terminal-host.service"
grep -q '^RuntimeDirectory=swarm$' "$SWARM_SYSTEMD_USER_ROOT/swarm-terminal-host.service"
if grep -q '^RuntimeDirectory=' "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"; then
  echo "API must not own the terminal host runtime directory" >&2
  exit 1
fi
[ -x "$SWARM_INSTALL_ROOT/current/swarm-package" ]
[ -d "$SWARM_STATE_ROOT/providers/claude" ]

# An ordinary app/API update repairs the stable worker bridge launcher without
# moving or restarting the independently pinned terminal host.
rm -f "$SWARM_BIN_ROOT/swarm-terminal-host"

# Development mode is explicit, checkout-scoped, same-port, and restarts only
# the replaceable API when it is enabled or disabled.
dev_checkout="$HOME/projects/swarm-next"
mkdir -p "$dev_checkout/packaging/linux"
cat > "$dev_checkout/packaging/linux/build-development-release.sh" <<'EOF'
#!/bin/sh
exit 1
EOF
chmod +x "$dev_checkout/packaging/linux/build-development-release.sh"
git -C "$dev_checkout" init -q
git -C "$dev_checkout" config user.name "Swarm Package Test"
git -C "$dev_checkout" config user.email "swarm-package-test@example.invalid"
git -C "$dev_checkout" add packaging/linux/build-development-release.sh
git -C "$dev_checkout" commit -qm "test development checkout"
: > "$HOME/systemctl.log"
"$package" enable-development "$dev_checkout"
grep -q "^SWARM_DEV_CHECKOUT=$dev_checkout$" "$SWARM_CONFIG_ROOT/swarm-dev.env"
grep -q "^SWARM_DEV_RELOAD_REQUEST_PATH=$SWARM_STATE_ROOT/development-reload.request$" "$SWARM_CONFIG_ROOT/swarm-dev.env"
grep -q "^SWARM_DEV_RELOAD_STATUS_PATH=$SWARM_STATE_ROOT/development-reload.status$" "$SWARM_CONFIG_ROOT/swarm-dev.env"
[ "$(cat "$SWARM_STATE_ROOT/development-reload.status")" = "state=idle" ]
grep -q "ReadWritePaths=$SWARM_INSTALL_ROOT $SWARM_STATE_ROOT $dev_checkout" "$SWARM_SYSTEMD_USER_ROOT/swarm-development-reload.service"
grep -q '^--user enable --now swarm-development-reload.path$' "$HOME/systemctl.log"
grep -q '^--user restart swarm-api.service$' "$HOME/systemctl.log"
if grep -q 'restart swarm-terminal-host.service' "$HOME/systemctl.log"; then
  echo "development enable touched the terminal host" >&2
  exit 1
fi
printf 'state=requested\n' > "$SWARM_STATE_ROOT/development-reload.status"
printf 'request\n' > "$SWARM_STATE_ROOT/development-reload.request"
mkdir -p "$SWARM_STATE_ROOT/development-build/stale-one" "$SWARM_STATE_ROOT/development-build/stale-two"
printf 'stale\n' > "$SWARM_STATE_ROOT/development-build/stale-one/file"
if "$package" reload-development; then
  echo "failing development build unexpectedly succeeded" >&2
  exit 1
fi
grep -q '^state=failed$' "$SWARM_STATE_ROOT/development-reload.status"
[ ! -e "$SWARM_STATE_ROOT/development-reload.request" ]
[ -z "$(find "$SWARM_STATE_ROOT/development-build" -mindepth 1 -maxdepth 1 -print -quit)" ]
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ]
: > "$HOME/systemctl.log"
"$package" disable-development
[ ! -e "$SWARM_CONFIG_ROOT/swarm-dev.env" ]
[ ! -e "$SWARM_STATE_ROOT/development-reload.status" ]
grep -q '^--user disable --now swarm-development-reload.path$' "$HOME/systemctl.log"
grep -q '^--user restart swarm-api.service$' "$HOME/systemctl.log"
if grep -q 'restart swarm-terminal-host.service' "$HOME/systemctl.log"; then
  echo "development disable touched the terminal host" >&2
  exit 1
fi
if command -v systemd-analyze >/dev/null 2>&1; then
  systemd-analyze --user verify \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-terminal-host.service" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-host-reconcile.service" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-host-reconcile.path" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-host-reconcile.timer" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm.target"
fi

# Compatible API/browser updates preserve an active sidecar and its sessions.
printf '1\n' > "$HOME/running-sessions"
printf 'database-v1\n' > "$SWARM_STATE_ROOT/swarm.sqlite3"
mkdir -p "$SWARM_STATE_ROOT/backups"
old_backup=1
while [ "$old_backup" -le 11 ]; do
  old_path="$SWARM_STATE_ROOT/backups/pre-update-old-$old_backup.sqlite3"
  printf 'old-%s\n' "$old_backup" > "$old_path"
  touch -d "@$old_backup" "$old_path"
  old_backup=$((old_backup + 1))
done
: > "$HOME/systemctl.log"
: > "$HOME/swarmctl.log"
"$package" update "$test_root/bundle-2.0.0"
[ -x "$SWARM_BIN_ROOT/swarm-terminal-host" ]
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/previous/VERSION")" = "1.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "1.0.0" ]
grep -q '^--user restart swarm-api.service$' "$HOME/systemctl.log"
if grep -Eq 'stop swarm.target|restart swarm-terminal-host.service' "$HOME/systemctl.log"; then
  echo "compatible update touched the terminal host" >&2
  exit 1
fi
if grep -Eq '^drain$|^wait-ready$' "$HOME/swarmctl.log"; then
  echo "compatible update drained active workers" >&2
  exit 1
fi
[ -f "$SWARM_INSTALL_ROOT/assets/app-1.0.0.js" ]
[ -f "$SWARM_INSTALL_ROOT/assets/app-2.0.0.js" ]
[ "$(find "$SWARM_INSTALL_ROOT/releases" -mindepth 1 -maxdepth 1 -type d | wc -l)" -eq 2 ]
[ "$(cat "$SWARM_STATE_ROOT/backups/pre-update-2.0.0.sqlite3")" = "database-v1" ]
[ "$(tail -n 1 "$HOME/verify-release.log")" = "2.0.0" ]
[ "$(find "$SWARM_STATE_ROOT/backups" -maxdepth 1 -type f -name 'pre-update-*.sqlite3' | wc -l)" -eq 10 ]
[ ! -e "$SWARM_STATE_ROOT/backups/pre-update-old-1.sqlite3" ]

# API rollback is also sidecar-safe while a worker is active.
: > "$HOME/systemctl.log"
"$package" rollback
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "1.0.0" ]
grep -q '^--user restart swarm-api.service$' "$HOME/systemctl.log"
if grep -q 'swarm-terminal-host.service' "$HOME/systemctl.log"; then
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
printf 'requested_at=%s\ntarget_version=2.0.0\n' "$(date +%s)" > "$SWARM_STATE_ROOT/worker-engine-maintenance.request"
printf '0\n' > "$HOME/running-sessions"
: > "$HOME/systemctl.log"
"$package" reconcile-host-requested
[ ! -e "$SWARM_STATE_ROOT/worker-engine-maintenance.request" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "2.0.0" ]
[ -x "$SWARM_BIN_ROOT/swarm-terminal-host" ]
grep -q '^--user restart swarm-terminal-host.service$' "$HOME/systemctl.log"

# Protocol changes fail closed against the independently pinned host.
if "$package" update "$test_root/bundle-4.0.0"; then
  echo "incompatible protocol update unexpectedly succeeded" >&2
  exit 1
fi
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ]

# A failed API health check restores only the previous API/browser pointer.
printf 'database-v2\n' > "$SWARM_STATE_ROOT/swarm.sqlite3"
if "$package" update "$test_root/bundle-3.0.0"; then
  echo "unhealthy update unexpectedly succeeded" >&2
  exit 1
fi
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "2.0.0" ]
[ "$(cat "$SWARM_STATE_ROOT/swarm.sqlite3")" = "database-v2" ]
[ "$(cat "$SWARM_STATE_ROOT/backups/pre-update-3.0.0.sqlite3")" = "database-v2" ]
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
grep -q '^--user stop swarm.target$' "$HOME/systemctl.log"
grep -q '^--user enable --now swarm.target$' "$HOME/systemctl.log"

# A failed protocol migration restores both independently pinned pointers.
printf 'database-v4\n' > "$SWARM_STATE_ROOT/swarm.sqlite3"
if "$package" migrate-protocol "$test_root/bundle-6.0.0"; then
  echo "unhealthy protocol migration unexpectedly succeeded" >&2
  exit 1
fi
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "4.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "4.0.0" ]
[ "$(cat "$SWARM_STATE_ROOT/swarm.sqlite3")" = "database-v4" ]
[ "$(cat "$SWARM_STATE_ROOT/backups/pre-update-6.0.0.sqlite3")" = "database-v4" ]

# Database restore verifies input, creates a rollback snapshot, restarts only
# the API, and preserves the terminal host and repository root.
printf 'old-database\n' > "$SWARM_STATE_ROOT/swarm.sqlite3"
printf 'restored-database\n' > "$HOME/hive-backup.sqlite3"
: > "$HOME/systemctl.log"
"$package" restore "$HOME/hive-backup.sqlite3"
[ "$(cat "$SWARM_STATE_ROOT/swarm.sqlite3")" = "restored-database" ]
grep -q '^--user stop swarm-api.service$' "$HOME/systemctl.log"
grep -q '^--user start swarm-api.service$' "$HOME/systemctl.log"
if grep -q 'swarm-terminal-host.service' "$HOME/systemctl.log"; then
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
[ ! -e "$SWARM_BIN_ROOT/swarm-terminal-host" ]
[ -f "$SWARM_STATE_ROOT/operator-data" ]
[ -f "$SWARM_CONFIG_ROOT/swarm.env" ]
printf 'package lifecycle smoke passed\n'
