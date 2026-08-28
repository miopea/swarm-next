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
# is-active ANSWERS HONESTLY, by tracking what this stub was told to start and
# stop. A stub that exits 0 for everything reports every unit as permanently
# ACTIVE, so a caller that verifies its own stop waits forever — which is what
# happened the first time swarm-package started checking, on 2026-08-28.
#
# It is worth saying why the stub had to grow this rather than the check being
# softened: the check exists because `systemctl stop` returned on the operator's
# machine while the terminal host was still running. A harness that cannot
# represent "this unit is still up" cannot test the thing that broke.
stub_state="$HOME/unit-state"
mkdir -p "$stub_state"
stub_verb=
stub_units=
stub_now=
for stub_argument in "$@"; do
  case "$stub_argument" in
    --now) stub_now=1; continue;;
    --*) continue;;
  esac
  if [ -z "$stub_verb" ]; then stub_verb=$stub_argument; else stub_units="$stub_units $stub_argument"; fi
done
stub_mark() { for stub_unit in $stub_units; do : > "$stub_state/$stub_unit"; done; }
stub_clear() { for stub_unit in $stub_units; do rm -f "$stub_state/$stub_unit"; done; }
case "$stub_verb" in
  start|restart) stub_mark;;
  enable) [ -z "$stub_now" ] || stub_mark;;
  stop) stub_clear;;
  disable) [ -z "$stub_now" ] || stub_clear;;
  is-active)
    for stub_unit in $stub_units; do [ -f "$stub_state/$stub_unit" ] || exit 3; done
    exit 0;;
esac
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
for field_tag in v0.8.17 v0.8.18 v0.8.19 v0.9.0; do
  field_package="$test_root/field-swarm-package"
  git -C "$repo_root" show "$field_tag:packaging/linux/swarm-package" > "$field_package" 2>/dev/null \
    || fail "could not extract $field_tag swarm-package — is the tag fetched?"
  chmod +x "$field_package"
  # The assumption this whole test rests on: a Hive of this vintage installs a
  # release by handing control to the NEW bundle's swarm-package. v0.8.x
  # hardcodes `update`; v0.9.0 selects a command into $apply_command and can
  # choose migrate-protocol-if-idle. Both are handoffs, and the check has to
  # admit both without ceasing to be a check — if a future tag stops handing
  # off at all, every assertion below is measuring the wrong thing.
  grep -qE 'swarm-package" (update|"\$apply_command") "\$requested"' "$field_package" \
    || fail "$field_tag no longer hands the install to the new bundle"

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

# --- WHAT A 0.9.0 HIVE ACTUALLY DOES WITH THIS RELEASE -----------------------
#
# The loop above runs with no sessions, which is the case that was never in
# doubt. A 0.9.0 Hive is different from the 0.8.x ones in exactly one way: its
# apply_release SELECTS a command, and picks migrate-protocol-if-idle when the
# protocols differ — the deferring path that cannot finish while an autostart
# worker keeps reviving. So the question this release turns on is what a 0.9.0
# Hive does with a SAME-protocol release while a worker is running.
field_package="$test_root/field-swarm-package"
git -C "$repo_root" show v0.9.0:packaging/linux/swarm-package > "$field_package" 2>/dev/null \
  || fail "could not extract v0.9.0 swarm-package"
chmod +x "$field_package"

rm -rf "$SWARM_INSTALL_ROOT" "$SWARM_CONFIG_ROOT" "$SWARM_STATE_ROOT" "$SWARM_SYSTEMD_USER_ROOT"
mkdir -p "$SWARM_STATE_ROOT"
make_bundle 1.0.0 6
sh "$test_root/bundle-1.0.0/swarm-package" install "$test_root/bundle-1.0.0" >/dev/null
cp "$field_package" "$SWARM_INSTALL_ROOT/current/swarm-package"
chmod +x "$SWARM_INSTALL_ROOT/current/swarm-package"

# Same protocol as the installed host, which is what 0.9.1 is to a 0.9.0 Hive.
make_bundle 2.0.0 6
mkdir -p "$SWARM_STATE_ROOT/downloads"
rm -rf "$SWARM_STATE_ROOT/downloads/2.0.0"
cp -r "$test_root/bundle-2.0.0" "$SWARM_STATE_ROOT/downloads/2.0.0"
# A worker that keeps coming back, which is what defeats the deferring path.
printf '3\n' > "$HOME/running-sessions"
# The log is cumulative and the first-time install wrote to it, so it is cleared
# here or the assertion below measures the install rather than this update.
: > "$HOME/systemctl.log"
printf '%s\n' "$SWARM_STATE_ROOT/downloads/2.0.0" > "$SWARM_STATE_ROOT/release-apply.request"
sh "$SWARM_INSTALL_ROOT/current/swarm-package" apply-release >/dev/null 2>&1 \
  || fail "a 0.9.0 Hive could not install a same-protocol release while a worker was running"

[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ] \
  || fail "a 0.9.0 Hive did not install a same-protocol release"
# THE POINT: no deferral was entered, so nothing is waiting on an idle that an
# autostart worker will never allow.
[ ! -f "$SWARM_STATE_ROOT/protocol-migration.pending" ] \
  || fail "a same-protocol release put a 0.9.0 Hive into the deferring path"
# And the workers were not taken away for an ordinary update.
if grep -q '^--user stop swarm.target$' "$HOME/systemctl.log" 2>/dev/null; then
  fail "a same-protocol release stopped the whole stack on a 0.9.0 Hive"
fi

printf 'a 0.9.0 Hive takes a same-protocol release with workers running\n'
