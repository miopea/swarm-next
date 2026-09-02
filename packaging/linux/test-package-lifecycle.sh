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
# is-failed ANSWERS HONESTLY, or the updater's start-limit check is untestable.
# A stub that exits 0 for everything reports every unit as failed, so the check
# fires on every start and the log looks identical whether it works or not.
# systemd's convention is non-zero for "not failed", which is the state a
# healthy box is in; the marker file is how a test asks for the other one.
for stub_argument in "$@"; do
  if [ "$stub_argument" = "is-failed" ]; then
    [ -f "$HOME/unit-latched-failed" ] || exit 1
    exit 0
  fi
done
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
  # A curl that SPEAKS THE CREDENTIAL BACK. No released curl is known to do
  # this for our config shape — that was measured — but the guarantee has to
  # hold for the next version of the tool, so the test asserts the channel is
  # closed rather than asserting curl's manners.
  if [ -f "$HOME/curl-leaks" ]; then
    printf 'curl: (26) Authorization: Bearer %s\n' "$(cat "$HOME/curl-leaks")" >&2
    exit 26
  fi
  cp "$SWARM_STATE_ROOT/swarm.sqlite3" "$output"
  exit 0
fi
version=$(cat "$SWARM_INSTALL_ROOT/current/VERSION")
printf '%s\n' "$version" >> "$HOME/curl.log"
[ "$version" != "3.0.0" ] || printf 'database-v3\n' > "$SWARM_STATE_ROOT/swarm.sqlite3"
[ "$version" != "6.0.0" ] || printf 'database-v6\n' > "$SWARM_STATE_ROOT/swarm.sqlite3"
[ "$version" != "3.0.0" ] && [ "$version" != "6.0.0" ] && [ "$version" != "8.0.0" ]
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


# THE TWO SERVICES THAT DEAL IN WORKSPACES MUST AGREE ABOUT THE BOUNDARY.
#
# ProtectHome=read-only makes the whole home read-only inside each namespace,
# so a workspace is only writable where ReadWritePaths says so. The terminal
# host ENFORCES that boundary and the API REPORTS on it, and a report made from
# a namespace where nothing under home is writable says EROFS for every
# workspace — marking every worker Blocked while the workers themselves run
# perfectly well.
#
# It shipped that way, and it hid on the machine it was written on because
# ~/projects there is a separate filesystem that ProtectHome does not cover. On
# the ordinary layout, where the workspace is a directory inside the same
# filesystem as home, every install was affected. Found on a fresh WSL install
# on 2026-08-26 after the EROFS was read as a failing disk and chased through
# dumpe2fs, dmesg and Windows free space. The disk was healthy throughout.
#
# Checked rather than remembered, for the same reason as the block above: a
# missing ReadWritePaths entry does not fail a unit, it produces one that starts
# and then cannot write, which reads as anything except a permission problem.
for unit in swarm-api swarm-terminal-host; do
  grep -q 'ReadWritePaths=@WORKSPACE_ROOT@' \
    "$repo_root/packaging/systemd-user/$unit.service.in" || {
    printf 'unit %s.service must be able to write @WORKSPACE_ROOT@\n' "$unit" >&2
    exit 1
  }
done

# THE TERMINAL HOST MUST NOT HAVE A READ-ONLY HOME.
#
# It runs the operator's coding agents, and they write across $HOME as a matter
# of course. ProtectHome=read-only there fails in the worst available way:
# ReadWritePaths on a single FILE is a bind mount of one inode, and a writer
# that saves by atomic replace — temp file, rename over the target, which is
# what Claude Code does for ~/.claude.json — detaches from that inode. The write
# reports success and the file never changes.
#
# On 2026-08-26 that produced a fresh install asking for login and full
# onboarding on every wake, with ~/.claude.json byte-identical throughout: 517
# bytes, same inode, same mtime. Credentials inside the ~/.claude DIRECTORY
# persisted fine, which is the tell — a bind-mounted directory works, a
# bind-mounted file does not.
#
# ProtectSystem=strict still guards the system tree. Home is what this service
# exists to work in.
grep -q '^ProtectHome=' "$repo_root/packaging/systemd-user/swarm-terminal-host.service.in" && {
  printf 'swarm-terminal-host.service must not restrict $HOME: it runs agents that write there\n' >&2
  exit 1
}

# And the API, which does NOT run agents, keeps its read-only home plus the one
# directory it genuinely writes into.
grep -q '^ProtectHome=read-only$' \
  "$repo_root/packaging/systemd-user/swarm-api.service.in" || {
  printf 'swarm-api.service should keep ProtectHome=read-only; it runs no agent code\n' >&2
  exit 1
}
grep -q 'ReadWritePaths=-%h/.claude$' \
  "$repo_root/packaging/systemd-user/swarm-api.service.in" || {
  printf 'swarm-api.service needs ~/.claude for resume history, tolerated when absent\n' >&2
  exit 1
}

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
    # A corrupt backup is otherwise unreachable from here, and the refusal it
    # produces is one of the failures the operator named as uninformative.
    [ ! -f "$HOME/verify-fails" ] || exit 1
  fi
  if [ "$command" = "status" ]; then
    running=0
    [ ! -f "$HOME/running-sessions" ] || running=$(cat "$HOME/running-sessions")
    # A host too old to classify OMITS these keys rather than reporting zero.
    # Modelled as absence on purpose: reading a missing key as "nobody is busy"
    # is the exact failure the three-way predicate exists to prevent.
    if [ -f "$HOME/host-cannot-report-busy" ]; then
      printf '{"protocol_version":5,"running_sessions":%s}\n' "$running"
    else
      busy=0
      [ ! -f "$HOME/busy-sessions" ] || busy=$(cat "$HOME/busy-sessions")
      unreadable=0
      [ ! -f "$HOME/unreadable-sessions" ] || unreadable=$(cat "$HOME/unreadable-sessions")
      printf '{"protocol_version":5,"running_sessions":%s,"busy_sessions":%s,"unreadable_sessions":%s}\n' \
        "$running" "$busy" "$unreadable"
    fi
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
make_bundle 7.0.0 7
make_bundle 8.0.0 8
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
grep -q 'ReadWritePaths=-%h/.claude$' "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
grep -q 'ReadWritePaths=-%h/.claude.json$' "$SWARM_SYSTEMD_USER_ROOT/swarm-api.service"
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
# The INVERSE of the swarm-api assertion above, and deliberately so. The API
# keeps ProtectHome=read-only, so it needs those two single-file binds to reach
# Claude's credentials at all. The terminal host dropped ProtectHome entirely,
# which makes the binds not merely unnecessary but harmful: a bind mount of one
# inode cannot survive a writer that renames a fresh file over the old one, so
# every credential refresh landed on a detached inode and every wake asked the
# worker to onboard again. Absence here is the fix, not an omission.
if grep -qE '^ReadWritePaths=-?%h/\.claude(\.json)?$' \
     "$SWARM_SYSTEMD_USER_ROOT/swarm-terminal-host.service"; then
  echo "swarm-terminal-host must NOT bind ~/.claude as single files -- a bind of" >&2
  echo "one inode breaks Claude's atomic credential writes; it has no ProtectHome" >&2
  echo "and reaches its home directly." >&2
  exit 1
fi
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
# A PROTOCOL CHANGE IS REFUSED BEFORE THE BUILD, NOT AFTER IT.
#
# A reload ends in `update`, which refuses a protocol change by design — and on
# 2026-08-27 that refusal arrived at the end of a five-minute build, every time,
# for three hours. The build script here exits 1, so if the refusal came late
# this test could not tell the two failures apart: the marker file is what
# proves the builder was never reached.
mkdir -p "$dev_checkout/crates/swarm-terminal/src"
printf 'pub const PROTOCOL_VERSION: u16 = 99;\n' > "$dev_checkout/crates/swarm-terminal/src/ipc.rs"
cat > "$dev_checkout/packaging/linux/build-development-release.sh" <<'EOF'
#!/bin/sh
touch "$HOME/the-builder-ran"
exit 1
EOF
chmod +x "$dev_checkout/packaging/linux/build-development-release.sh"
rm -f "$HOME/the-builder-ran"
protocol_refusal=$("$package" reload-development 2>&1 || true)
case "$protocol_refusal" in
  *migrate-protocol*) :;;
  *) echo "the reload refusal does not name migrate-protocol: $protocol_refusal" >&2; exit 1;;
esac
case "$protocol_refusal" in
  *"STOPS EVERY WORKER"*) :;;
  *) echo "the reload refusal does not say the migration stops workers" >&2; exit 1;;
esac
[ ! -f "$HOME/the-builder-ran" ] || { echo "the reload built before refusing" >&2; exit 1; }
grep -q '^step=protocol-change$' "$SWARM_STATE_ROOT/development-reload.status" \
  || { echo "the status did not record why the reload was refused" >&2; exit 1; }
# Back to a checkout whose protocol agrees, so the rest of this file is unaffected.
printf 'pub const PROTOCOL_VERSION: u16 = 5;\n' > "$dev_checkout/crates/swarm-terminal/src/ipc.rs"
cat > "$dev_checkout/packaging/linux/build-development-release.sh" <<'EOF'
#!/bin/sh
exit 1
EOF
chmod +x "$dev_checkout/packaging/linux/build-development-release.sh"

cat > "$dev_checkout/packaging/linux/build-development-release.sh" <<'EOF'
#!/bin/sh
echo "   Compiling swarm-api v0.0.0" >&2
echo "error[E0433]: failed to resolve: use of undeclared crate or module \`nope\`" >&2
echo "error: could not compile \`swarm-api\` (lib) due to 1 previous error" >&2
exit 1
EOF
chmod +x "$dev_checkout/packaging/linux/build-development-release.sh"

printf 'state=requested\n' > "$SWARM_STATE_ROOT/development-reload.status"
printf 'request\n' > "$SWARM_STATE_ROOT/development-reload.request"
mkdir -p "$SWARM_STATE_ROOT/development-build/stale-one" "$SWARM_STATE_ROOT/development-build/stale-two"
printf 'stale\n' > "$SWARM_STATE_ROOT/development-build/stale-one/file"
if "$package" reload-development; then
  echo "failing development build unexpectedly succeeded" >&2
  exit 1
fi
grep -q '^state=failed$' "$SWARM_STATE_ROOT/development-reload.status"

# WHY THIS IS ASSERTED SEPARATELY FROM state=failed. Every reload failure used
# to write the same status, so the control room said "did not compile" whether
# the compiler had spoken or not — and on 2026-08-27 it said exactly that about
# a build that compiled fine and was refused at install. state=failed is the
# fact; reason and detail are what make it actionable without journalctl.
grep -q '^step=build$' "$SWARM_STATE_ROOT/development-reload.status" \
  || { echo "a failed build did not record step=build" >&2; exit 1; }
grep -q '^detail=.*E0433' "$SWARM_STATE_ROOT/development-reload.status" \
  || { echo "the status did not carry the compiler's own error line" >&2; exit 1; }
# One line, no newlines: the file is parsed as key=value, and a detail carrying
# a newline would silently become a key of its own.
[ "$(wc -l < "$SWARM_STATE_ROOT/development-reload.status")" -eq 5 ] \
  || { echo "the failure status is not five lines" >&2; exit 1; }
grep -q '^changed=nothing$' "$SWARM_STATE_ROOT/development-reload.status" \
  || { echo "a failed build did not record that nothing changed" >&2; exit 1; }

# A build that COMPILES and is refused at install must not be called a compile
# error. This is the case the operator hit.
cat > "$dev_checkout/packaging/linux/build-development-release.sh" <<'EOF'
#!/bin/sh
bundle="$1/refused"
mkdir -p "$bundle"
cat > "$bundle/swarm-package" <<'INNER'
#!/bin/sh
echo "swarm-package: this release speaks terminal-host protocol 10 and the installed host speaks 9" >&2
exit 1
INNER
chmod +x "$bundle/swarm-package"
echo "$bundle"
EOF
chmod +x "$dev_checkout/packaging/linux/build-development-release.sh"
printf 'state=requested\n' > "$SWARM_STATE_ROOT/development-reload.status"
printf 'request\n' > "$SWARM_STATE_ROOT/development-reload.request"
if "$package" reload-development; then
  echo "a refused install unexpectedly succeeded" >&2
  exit 1
fi
grep -q '^step=install$' "$SWARM_STATE_ROOT/development-reload.status" \
  || { echo "a refused install was not recorded as an install failure" >&2; exit 1; }
grep -q '^detail=.*protocol 10 and the installed host speaks 9' "$SWARM_STATE_ROOT/development-reload.status" \
  || { echo "the status did not carry the installer's own refusal" >&2; exit 1; }

# Back to a plainly failing builder for the state the rest of this file expects.
cat > "$dev_checkout/packaging/linux/build-development-release.sh" <<'EOF'
#!/bin/sh
exit 1
EOF
chmod +x "$dev_checkout/packaging/linux/build-development-release.sh"
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
# A session EXISTING is no longer what defers a reconcile — being mid-turn is.
# The reconcile assertions below are about refusing to stop live work, so the
# stub has to report live work rather than merely a session.
printf '1\n' > "$HOME/busy-sessions"
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
if grep -Eq 'stop .*swarm\.target|stop .*swarm-terminal-host\.service|restart swarm-terminal-host\.service' "$HOME/systemctl.log"; then
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
# THE ROLLBACK TARGET VERIFIES, NOT THE INCOMING RELEASE. This asserted 2.0.0
# until 2026-08-27, which pinned the defect rather than the requirement:
# verify-database migrates what it opens, so verifying with the new release
# rewrote the pre-update backup to the new schema and left the rollback holding
# a database its own binary refuses. See create_update_backup.
[ "$(tail -n 1 "$HOME/verify-release.log")" = "1.0.0" ]
[ "$(find "$SWARM_STATE_ROOT/backups" -maxdepth 1 -type f -name 'pre-update-*.sqlite3' | wc -l)" -eq 10 ]
[ ! -e "$SWARM_STATE_ROOT/backups/pre-update-old-1.sqlite3" ]

# A BACKUP THAT CANNOT BE VERIFIED SAYS WHAT TO DO NEXT.
#
# The operator named this one directly: "a pre-update database backup failed
# ... which named neither the cause nor a way forward". Naming the cause and
# stopping is only half an answer — a person standing in front of a refused
# install needs the next command, and this one is a plain cp they can read and
# decide about.
touch "$HOME/verify-fails"
backup_refusal=$("$package" update "$test_root/bundle-5.0.0" 2>&1 || true)
rm -f "$HOME/verify-fails"
case "$backup_refusal" in
  *"backup failed verification"*) :;;
  *) echo "the backup refusal does not name the cause: $backup_refusal" >&2; exit 1;;
esac
case "$backup_refusal" in
  *"cp $SWARM_STATE_ROOT/swarm.sqlite3"*) :;;
  *) echo "the backup refusal does not name a way forward: $backup_refusal" >&2; exit 1;;
esac
# It stopped BEFORE changing anything, and says so rather than leaving the
# reader to guess which half of an update they are standing in.
case "$backup_refusal" in
  *"stopped before changing anything"*) :;;
  *) echo "the backup refusal does not say whether anything changed" >&2; exit 1;;
esac
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" != "5.0.0" ] \
  || { echo "a refused backup still installed the release" >&2; exit 1; }

# API rollback is also sidecar-safe while a worker is active.
# A unit latched failed by an earlier crash loop is cleared before starting.
# Without this an update cannot install its way out: systemd answers every
# start with "start request repeated too quickly" and the updater reports a
# failure that has nothing to do with the release being installed.
: > "$HOME/systemctl.log"
touch "$HOME/unit-latched-failed"
"$package" rollback
grep -q '^--user reset-failed swarm-api.service$' "$HOME/systemctl.log"
rm -f "$HOME/unit-latched-failed"
# And a healthy unit is left alone rather than reset on every start.
: > "$HOME/systemctl.log"
"$package" update "$test_root/bundle-2.0.0"
if grep -q 'reset-failed' "$HOME/systemctl.log"; then
  echo "a healthy unit was reset anyway" >&2
  exit 1
fi
"$package" rollback
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "1.0.0" ]
grep -q '^--user restart swarm-api.service$' "$HOME/systemctl.log"
if grep -q 'swarm-terminal-host.service' "$HOME/systemctl.log"; then
  echo "API rollback touched the terminal host" >&2
  exit 1
fi
"$package" update "$test_root/bundle-2.0.0"

# THE CREDENTIAL-BEARING COMMAND'S OUTPUT NEVER REACHES THE CAPTURED STREAM.
#
# e74affc turned a failing step's stderr into `detail`, a field rendered on a
# card. curl is called with --config naming a file holding the operator token,
# so every byte curl prints on a failure now travels somewhere it did not
# before. This asserts the CHANNEL is closed, not that curl behaves: the stub
# speaks a token back deliberately, which no real curl was observed doing.
printf 'swarm_tok_TESTONLY_%s\n' "aaaabbbbcccc" > "$HOME/curl-leaks"
leak_probe=$(cat "$HOME/curl-leaks")
: > "$HOME/systemctl.log"
update_noise=$("$package" update "$test_root/bundle-5.0.0" 2>&1 || true)
rm -f "$HOME/curl-leaks"
case "$update_noise" in
  *"$leak_probe"*)
    echo "the credential-bearing command's output reached the captured stream" >&2
    exit 1;;
esac
# AND THE OPERATOR IS NOT LEFT WITH LESS. curl's exit code is translated into
# curl's own vocabulary, so the failure still says what happened.
case "$update_noise" in
  *"a local file could not be read or written (curl exit 26)"*) :;;
  *) echo "the withheld curl failure did not say what happened: $update_noise" >&2; exit 1;;
esac
case "$update_noise" in
  *"output is withheld because this request carries the operator token"*) :;;
  *) echo "the withholding was not explained" >&2; exit 1;;
esac
# The update still completed, because a failed API backup falls back to a copy.
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "5.0.0" ] \
  || { echo "withholding curl's output broke the update" >&2; exit 1; }
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
# THE THREE-WAY PREDICATE, DEMONSTRATED IN ALL THREE DIRECTIONS.
#
# The old test was a session COUNT, which autostart keeps above zero forever —
# so the deferral could never end and requested maintenance never landed.

# Each case needs a REAL pending engine change to reach the predicate at all:
# once host-current agrees with current there is nothing to reconcile and the
# script returns long before the session check. A proceeding case consumes that
# divergence, so it is restored before each one.
diverged_host_link=$(readlink "$SWARM_INSTALL_ROOT/host-current")
restore_pending_engine_change() {
  ln -sfn "$diverged_host_link" "$SWARM_INSTALL_ROOT/host-current"
}

# 1. MID-TURN DEFERS. Sessions exist AND one is working.
restore_pending_engine_change
printf '3\n' > "$HOME/running-sessions"
printf '1\n' > "$HOME/busy-sessions"
printf '0\n' > "$HOME/unreadable-sessions"
: > "$HOME/swarmctl.log"
reconcile_out=$("$package" reconcile-host-if-idle 2>&1)
printf '%s' "$reconcile_out" | grep -q 'mid-turn' \
  || { echo "a mid-turn worker did not defer the reconcile: $reconcile_out" >&2; exit 1; }
grep -q '^cancel-drain$' "$HOME/swarmctl.log" \
  || { echo "a deferred reconcile left the host drained" >&2; exit 1; }

# 2. RESTING SESSIONS PROCEED, SILENTLY. Three sessions, none working, all
#    readable. This is the case the old predicate could never reach, and the
#    silence is the assertion: a check that always warns says nothing on the
#    day it matters.
restore_pending_engine_change
printf '3\n' > "$HOME/running-sessions"
printf '0\n' > "$HOME/busy-sessions"
printf '0\n' > "$HOME/unreadable-sessions"
quiet_out=$("$package" reconcile-host-if-idle 2>&1)
# BOTH halves, because silence alone does not distinguish "proceeded quietly"
# from "deferred quietly" — and deferring quietly is the original bug.
printf '%s' "$quiet_out" | grep -q 'now uses' \
  || { echo "resting sessions did not let the engine update land: $quiet_out" >&2; exit 1; }
printf '%s' "$quiet_out" | grep -qi 'WARNING' \
  && { echo "a fully readable idle host warned about nothing: $quiet_out" >&2; exit 1; }

# 3a. A PROVIDER THIS BUILD CANNOT READ: DEFER, and name it. Not proceed —
#     the worker engine card is the deliberate route, and it exists.
restore_pending_engine_change
printf '0\n' > "$HOME/busy-sessions"
printf '2\n' > "$HOME/unreadable-sessions"
unreadable_out=$("$package" reconcile-host-if-idle 2>&1)
printf '%s' "$unreadable_out" | grep -q 'deferred.*cannot read' \
  || { echo "an unreadable provider did not defer: $unreadable_out" >&2; exit 1; }
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "1.0.0" ] \
  || { echo "an unreadable provider swapped the engine anyway" >&2; exit 1; }

# 3b. A HOST TOO OLD TO ANSWER omits the keys entirely. THIS IS THE CASE THAT
#     COST 12 LIVE SESSIONS: on the first release that adds the field, the
#     running host is ALWAYS the old one, so this fires with certainty.
restore_pending_engine_change
: > "$HOME/host-cannot-report-busy"
skew_out=$("$package" reconcile-host-if-idle 2>&1)
printf '%s' "$skew_out" | grep -q 'deferred.*cannot report which sessions are busy' \
  || { echo "an unanswerable host was treated as idle: $skew_out" >&2; exit 1; }
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "1.0.0" ] \
  || { echo "an unanswerable host had its engine swapped underneath it" >&2; exit 1; }

# 3c. AND THE OPERATOR'S DELIBERATE ROUTE STILL WORKS while it cannot check —
#     otherwise deferring would strand the upgrade instead of scheduling it.
"$package" reconcile-host >/dev/null 2>&1 \
  && { echo "required-mode reconcile should refuse while unverifiable" >&2; exit 1; }
rm -f "$HOME/host-cannot-report-busy"
printf '0\n' > "$HOME/unreadable-sessions"
# Each of the proceeding cases above CONSUMED the pending engine change, so put
# it back: what follows is the requested-maintenance test and it needs one.
restore_pending_engine_change

# THE CARD MUST BE A ROUTE OUT WHEN THE HOST CANNOT REPORT BUSY SESSIONS.
#
# Nothing covered this pair, and that is why it shipped. The suite asserted
# required-mode refusal (above) and requested-mode success with a host that CAN
# report (below), and the operator's real case was the cell in between: a host
# too old to answer plus the card asking. The timer deferred and the card died,
# so the engine could not update by any route.
#
# It is the FIRST upgrade to a build that reports busy sessions, because the
# host running at that moment is by definition the older one. Measured on the
# operator's WSL Hive 2026-09-02: app and API on 1.2.0, engine stuck on 1.1.1.
restore_pending_engine_change
: > "$HOME/host-cannot-report-busy"
printf 'requested_at=%s\ntarget_version=2.0.0\n' "$(date +%s)" > "$SWARM_STATE_ROOT/worker-engine-maintenance.request"
printf '0\n' > "$HOME/running-sessions"
"$package" reconcile-host-requested 2>"$HOME/requested-unverifiable.log" \
  || { echo "the card must proceed when the host cannot report busy sessions" >&2; exit 1; }
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "2.0.0" ] \
  || { echo "the engine did not actually swap on the card's request" >&2; exit 1; }
grep -q "cannot report which sessions are busy" "$HOME/requested-unverifiable.log" \
  || { echo "proceeding silently is the failure the operator ruled against" >&2; exit 1; }
rm -f "$HOME/host-cannot-report-busy"

restore_pending_engine_change
printf 'requested_at=%s\ntarget_version=2.0.0\n' "$(date +%s)" > "$SWARM_STATE_ROOT/worker-engine-maintenance.request"
printf '0\n' > "$HOME/running-sessions"
: > "$HOME/systemctl.log"
"$package" reconcile-host-requested
[ ! -e "$SWARM_STATE_ROOT/worker-engine-maintenance.request" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "2.0.0" ]
[ -x "$SWARM_BIN_ROOT/swarm-terminal-host" ]
grep -q '^--user restart swarm-terminal-host.service$' "$HOME/systemctl.log"

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

# --- A PROTOCOL CHANGE INSTALLS IN ONE HOP, FROM THE ENTRY POINT A HIVE USES --
#
# THE REASON THIS IS `update` AND NOT `migrate-protocol`: a Hive installs a
# release by handing control to the NEW bundle, and every version in the field
# ultimately runs `"$requested/swarm-package" update "$requested"` --
# v0.8.19 hardcodes exactly that and never selects migrate-protocol. So `update`
# is the only door a release can arrive through on the Hives that already exist.
# While it refused here, a protocol-bumping release was uninstallable in the
# field and the only alternative was a two-stage rollout: ship one release to
# teach every Hive, wait for everyone to take it, then ship the real one. The
# operator: "I have to count on a bunch of people to run an update, report back,
# and then I release another update and they update ... there has to be a better
# way."
: > "$HOME/systemctl.log"
"$package" update "$test_root/bundle-4.0.0" \
  || { echo "a protocol-bumping release could not be installed by update" >&2; exit 1; }

# THE INVARIANT THE OLD REFUSAL WAS PROTECTING, asserted directly rather than
# via the refusal it happened to use. A host and an API speaking different
# protocols is the failure; the guard is not weakened by moving them TOGETHER.
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "4.0.0" ] \
  || { echo "the API was not moved by the protocol update" >&2; exit 1; }
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "4.0.0" ] \
  || { echo "the host was left behind the API — the divergence the guard exists to prevent" >&2; exit 1; }
[ "$(cat "$SWARM_INSTALL_ROOT/current/PROTOCOL")" = "$(cat "$SWARM_INSTALL_ROOT/host-current/PROTOCOL")" ] \
  || { echo "the API and host protocols disagree after an update" >&2; exit 1; }
# The engine really was swapped, not just relinked.
grep -q '^--user enable --now swarm.target$' "$HOME/systemctl.log" \
  || { echo "the protocol update did not restart the stack" >&2; exit 1; }
# And rollback is still possible.
[ "$(cat "$SWARM_INSTALL_ROOT/previous/VERSION")" = "2.0.0" ] \
  || { echo "the protocol update did not retain a rollback target" >&2; exit 1; }

# IT SAYS WHAT IT COSTS. `update` otherwise promises workers keep running, and
# a protocol change cannot keep that promise. Announcing it is the difference
# between a surprise and a warned restart.
protocol_notice=$("$package" update "$test_root/bundle-7.0.0" 2>&1)
case "$protocol_notice" in
  *"changes the terminal-host protocol from 6 to 7"*) :;;
  *) echo "the protocol update did not name both protocols: $protocol_notice" >&2; exit 1;;
esac
case "$protocol_notice" in
  *"every worker session ends"*) :;;
  *) echo "the protocol update did not say it ends worker sessions" >&2; exit 1;;
esac
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "7.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "7.0.0" ]

# --- AND A BOTCHED ONE ROLLS BOTH BACK -------------------------------------
#
# "we have botched updates MANY times ... this needs to be certain." The risk
# of doing the migration inside `update` is a half-applied stack, so the
# unhealthy case is asserted on BOTH links, not just the API.
printf 'database-v7
' > "$SWARM_STATE_ROOT/swarm.sqlite3"
if "$package" update "$test_root/bundle-8.0.0"; then
  echo "an unhealthy protocol update unexpectedly succeeded" >&2
  exit 1
fi
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "7.0.0" ] \
  || { echo "a failed protocol update left the API moved" >&2; exit 1; }
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "7.0.0" ] \
  || { echo "a failed protocol update left the host moved" >&2; exit 1; }
[ "$(cat "$SWARM_INSTALL_ROOT/current/PROTOCOL")" = "$(cat "$SWARM_INSTALL_ROOT/host-current/PROTOCOL")" ] \
  || { echo "a failed protocol update left the stack half-applied" >&2; exit 1; }
[ "$(cat "$SWARM_STATE_ROOT/swarm.sqlite3")" = "database-v7" ] \
  || { echo "a failed protocol update did not restore the database" >&2; exit 1; }

# THE MIGRATION PREDICATE, ALL FOUR DIRECTIONS.
#
# This is the OPPOSITE arm to the reconcile on "unreadable": a protocol
# migration ends every session and the card's force path is a human route out,
# so deferring strands nothing while proceeding would guess. The reconcile has
# no such route, which is why it proceeds instead.

# bundle-4.0.0 (protocol 6) AGAINST AN INSTALLED PROTOCOL 7, deliberately.
# This used to migrate toward bundle-6.0.0, which is ALSO protocol 7 — so the
# refusal it asserted came from "protocol is unchanged" and the active-worker
# check was never reached. The assertion passed for the wrong reason. Every
# case below therefore also asserts WHY it refused.
migration_bundle="$test_root/bundle-4.0.0"

# Explicit migrate-protocol refuses a MID-TURN worker, and says so.
printf '1\n' > "$HOME/running-sessions"
printf '1\n' > "$HOME/busy-sessions"
printf '0\n' > "$HOME/unreadable-sessions"
migrate_busy=$("$package" migrate-protocol "$migration_bundle" 2>&1 || true)
printf '%s' "$migrate_busy" | grep -q 'mid-turn' \
  || { echo "a mid-turn worker did not defer the migration: $migrate_busy" >&2; exit 1; }

# A PROVIDER THIS BUILD CANNOT READ DEFERS HERE — the OPPOSITE arm to the
# reconcile, because the card's force path is a human route out.
printf '3\n' > "$HOME/running-sessions"
printf '0\n' > "$HOME/busy-sessions"
printf '2\n' > "$HOME/unreadable-sessions"
migrate_unreadable=$("$package" migrate-protocol "$migration_bundle" 2>&1 || true)
printf '%s' "$migrate_unreadable" | grep -q 'cannot read' \
  || { echo "an unreadable provider did not defer the migration: $migrate_unreadable" >&2; exit 1; }

# A HOST THAT CANNOT ANSWER defers too. Absent is not zero.
: > "$HOME/host-cannot-report-busy"
migrate_skew=$("$package" migrate-protocol "$migration_bundle" 2>&1 || true)
printf '%s' "$migrate_skew" | grep -q 'cannot report which sessions are busy' \
  || { echo "an unanswerable host did not defer the migration: $migrate_skew" >&2; exit 1; }
rm -f "$HOME/host-cannot-report-busy"

# AND THE NEGATIVE: everything readable and resting MIGRATES, silently.
# Asserting the migration HAPPENED rather than that nothing complained —
# a deferral is also silent, so silence alone would pass against the bug.
printf '3\n' > "$HOME/running-sessions"
printf '0\n' > "$HOME/busy-sessions"
printf '0\n' > "$HOME/unreadable-sessions"
"$package" migrate-protocol "$migration_bundle" >/dev/null 2>&1 \
  || { echo "a resting host refused a protocol migration" >&2; exit 1; }
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "4.0.0" ] \
  || { echo "the migration reported success without moving the host" >&2; exit 1; }

# Put the stack back where the next assertions expect it: they are about a
# 7 -> 8 update and need protocol 7 installed. bundle-4.0.0 is protocol 6, so
# this is the same 6 -> 7 hop the harness already exercises above.
"$package" update "$test_root/bundle-7.0.0" >/dev/null 2>&1 \
  || { echo "could not restore protocol 7 after the migration cases" >&2; exit 1; }
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "7.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "7.0.0" ]
printf '0
' > "$HOME/running-sessions"
: > "$HOME/systemctl.log"
"$package" update "$test_root/bundle-4.0.0"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "4.0.0" ]
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "4.0.0" ]

[ "$(cat "$SWARM_INSTALL_ROOT/previous/VERSION")" = "7.0.0" ]
grep -qE '^--user stop .*swarm-terminal-host\.service' "$HOME/systemctl.log" \
  || { echo "the migration did not stop the terminal host by name" >&2; exit 1; }
grep -qE '^--user stop .*swarm\.target' "$HOME/systemctl.log" \
  || { echo "the migration did not stop the target" >&2; exit 1; }
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
