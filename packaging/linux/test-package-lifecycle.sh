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
version=$(cat "$SWARM_INSTALL_ROOT/current/VERSION")
[ "$version" != "3.0.0" ]
EOF
chmod +x "$SWARM_SYSTEMCTL_BIN" "$SWARM_CURL_BIN"

make_bundle() {
  version=$1
  protocol=${2:-5}
  bundle="$test_root/bundle-$version"
  mkdir -p "$bundle/bin" "$bundle/web" "$bundle/systemd-user"
  for binary in swarm-api swarm-terminal-host swarmctl; do
    cat > "$bundle/bin/$binary" <<'EOF'
#!/bin/sh
if [ "$(basename "$0")" = "swarmctl" ]; then
  printf '%s\n' "$1" >> "$HOME/swarmctl.log"
  [ ! -e "$HOME/fail-ready" ] || [ "$1" != "wait-ready" ] || exit 3
fi
exit 0
EOF
    chmod +x "$bundle/bin/$binary"
  done
  printf '<!doctype html><title>test</title>\n' > "$bundle/web/index.html"
  cp "$repo_root/packaging/systemd-user/"*.in "$bundle/systemd-user/"
  printf '%s\n' "$version" > "$bundle/VERSION"
  printf '%s\n' "$protocol" > "$bundle/PROTOCOL"
  (cd "$bundle" && find bin web systemd-user -type f -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS)
}

make_bundle 1.0.0
make_bundle 2.0.0
make_bundle 3.0.0
make_bundle 4.0.0 6
make_bundle 5.0.0
package="$repo_root/packaging/linux/swarm-next-package"

"$package" install "$test_root/bundle-1.0.0"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ]
[ -f "$SWARM_CONFIG_ROOT/swarm-next.env" ]
[ "$(stat -c %a "$SWARM_CONFIG_ROOT/swarm-next.env")" = "600" ]
grep -q '127.0.0.1:8766' "$SWARM_CONFIG_ROOT/swarm-next.env"
grep -q "SWARM_WORKSPACE_ROOTS=$SWARM_WORKSPACE_ROOT" "$SWARM_CONFIG_ROOT/swarm-next.env"
grep -q "$SWARM_INSTALL_ROOT/current/bin/swarm-api" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service"
grep -q "SWARM_DATABASE_PATH=$SWARM_STATE_ROOT/swarm-next.sqlite3" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service"
grep -q "ReadWritePaths=$SWARM_STATE_ROOT" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service"
grep -q "CLAUDE_CONFIG_DIR=$SWARM_STATE_ROOT/providers/claude" "$SWARM_SYSTEMD_USER_ROOT/swarm-next-terminal-host.service"
grep -q 'PATH=%h/.local/bin:/usr/local/bin:/usr/bin:/bin' "$SWARM_SYSTEMD_USER_ROOT/swarm-next-terminal-host.service"
grep -q '^RuntimeDirectory=swarm-next$' "$SWARM_SYSTEMD_USER_ROOT/swarm-next-terminal-host.service"
if grep -q '^RuntimeDirectory=' "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service"; then
  echo "API must not own the terminal host runtime directory" >&2
  exit 1
fi
[ -d "$SWARM_STATE_ROOT/providers/claude" ]
if command -v systemd-analyze >/dev/null 2>&1; then
  systemd-analyze --user verify \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-next-terminal-host.service" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-next-api.service" \
    "$SWARM_SYSTEMD_USER_ROOT/swarm-next.target"
fi

if "$package" update "$test_root/bundle-4.0.0"; then
  echo "incompatible protocol update unexpectedly succeeded" >&2
  exit 1
fi
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ]

touch "$HOME/fail-ready"
if "$package" update "$test_root/bundle-5.0.0"; then
  echo "non-ready update unexpectedly succeeded" >&2
  exit 1
fi
rm "$HOME/fail-ready"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ]
grep -q '^cancel-drain$' "$HOME/swarmctl.log"

"$package" update "$test_root/bundle-2.0.0"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/previous/VERSION")" = "1.0.0" ]

"$package" rollback
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ]
"$package" update "$test_root/bundle-2.0.0"

if "$package" update "$test_root/bundle-3.0.0"; then
  echo "unhealthy update unexpectedly succeeded" >&2
  exit 1
fi
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ]

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
