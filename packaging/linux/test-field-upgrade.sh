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

# --- the release-apply path -------------------------------------------------
#
# What the control room's Install button actually triggers. The API writes a
# request file naming a verified download; a systemd path unit then runs
# `swarm-package apply-release`. Everything below the unit is exercised here.

fail() { printf 'test-field-upgrade: %s\n' "$1" >&2; exit 1; }

# --- THE FIELD UPGRADE: a real v0.8.19 Hive takes a protocol change ---------
#
# The operator, 2026-08-28: "we have botched updates MANY times ... so this
# needs to be certain, and when it is ready and deployed I can run it on my
# local version of WSL which I'm guessing is older."
#
# Every other test here installs using the CURRENT swarm-package, which is not
# what a developer has. This one drives the ACTUAL script from the v0.8.19 tag
# as the installed Hive, because that is the version in the field and its
# apply_release hardcodes `update` and never selects migrate-protocol:
#
#     packaging/linux/swarm-package@v0.8.19:  "$requested/swarm-package" update "$requested"
#
# So the only thing that can rescue a protocol change is the NEW bundle's
# `update`. If this test passes, a developer on 0.8.19 installs a protocol
# change in ONE hop with no coordination. If it fails, they are stranded and no
# amount of care in the new release helps them.
# EVERY VERSION IN THE FIELD, not just the newest. The operator's WSL box is on
# 0.8.17 and the devs are on 0.8.19; all three release lines speak protocol 9
# and all three hardcode `update` in apply_release, so one fix has to cover the
# lot — and that is asserted here rather than assumed from reading one of them.
for field_tag in v0.8.17 v0.8.18 v0.8.19; do
  field_package="$test_root/field-swarm-package"
  git -C "$repo_root" show "$field_tag:packaging/linux/swarm-package" > "$field_package" 2>/dev/null \
    || fail "could not extract $field_tag swarm-package — is the tag fetched?"
  chmod +x "$field_package"
  # The assumption this whole test rests on: a Hive of this vintage installs a
  # release by handing control to the NEW bundle's `update`, and never selects
  # migrate-protocol itself.
  grep -q 'update "\$requested"' "$field_package" \
    || fail "$field_tag no longer matches the assumption this test is built on"

  # A clean Hive for each vintage, or the second run inherits the first's fix.
  rm -rf "$SWARM_INSTALL_ROOT" "$SWARM_CONFIG_ROOT" "$SWARM_STATE_ROOT" "$SWARM_SYSTEMD_USER_ROOT"
  mkdir -p "$SWARM_STATE_ROOT"

  make_bundle 1.0.0
  sh "$test_root/bundle-1.0.0/swarm-package" install "$test_root/bundle-1.0.0" >/dev/null
  # Make the INSTALLED package that vintage's, which is the state a developer's
  # machine is actually in.
  cp "$field_package" "$SWARM_INSTALL_ROOT/current/swarm-package"
  chmod +x "$SWARM_INSTALL_ROOT/current/swarm-package"

  # A release that bumps the terminal-host protocol, the way the next real one will.
  make_bundle 2.0.0 6
  mkdir -p "$SWARM_STATE_ROOT/downloads"
  rm -rf "$SWARM_STATE_ROOT/downloads/2.0.0"
  cp -r "$test_root/bundle-2.0.0" "$SWARM_STATE_ROOT/downloads/2.0.0"
  printf '%s\n' "$SWARM_STATE_ROOT/downloads/2.0.0" > "$SWARM_STATE_ROOT/release-apply.request"

  # Driven by that vintage's script, exactly as swarm-release-apply.service would.
  sh "$SWARM_INSTALL_ROOT/current/swarm-package" apply-release >/dev/null 2>&1 \
    || fail "a $field_tag Hive could not install a protocol-bumping release in one hop"

  [ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ] \
    || fail "$field_tag: the API was not upgraded by the field install"
  [ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "2.0.0" ] \
    || fail "$field_tag: the host was left behind — a developer would be on mismatched protocols"
  [ "$(cat "$SWARM_INSTALL_ROOT/current/PROTOCOL")" = "$(cat "$SWARM_INSTALL_ROOT/host-current/PROTOCOL")" ] \
    || fail "$field_tag: the field install left the API and host on different protocols"
  [ "$(cat "$SWARM_INSTALL_ROOT/current/PROTOCOL")" = "6" ] \
    || fail "$field_tag: the field install did not reach the new protocol"

  # And the Hive is now running a swarm-package that knows about migrations, so
  # the NEXT protocol change is handled by the improved path rather than this one.
  grep -q 'migrate-protocol-if-idle' "$SWARM_INSTALL_ROOT/current/swarm-package" \
    || fail "$field_tag: the installed package did not become the new one"

  printf 'field upgrade from %s passed\n' "$field_tag"
done
