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

fail() { printf 'test-release-apply: %s\n' "$1" >&2; exit 1; }

make_bundle 1.0.0
sh "$test_root/bundle-1.0.0/swarm-package" install "$test_root/bundle-1.0.0" >/dev/null
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ] || fail "1.0.0 did not install"

# A downloaded, verified release is left where the API puts it.
make_bundle 2.0.0
mkdir -p "$SWARM_STATE_ROOT/downloads"
cp -r "$test_root/bundle-2.0.0" "$SWARM_STATE_ROOT/downloads/2.0.0"

# 1. A request naming something outside the download root is refused. A local
#    attacker able to write here already owns the token, but a bug that writes
#    the wrong path must not become an arbitrary installer.
printf '%s\n' "$test_root/bundle-2.0.0" > "$SWARM_STATE_ROOT/release-apply.request"
if sh "$SWARM_INSTALL_ROOT/current/swarm-package" apply-release >/dev/null 2>&1; then
  fail "a request outside the download root was accepted"
fi
grep -q 'state=refused' "$SWARM_STATE_ROOT/release-apply.status" || fail "refusal was not reported"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ] || fail "a refused request still changed the release"
[ ! -f "$SWARM_STATE_ROOT/release-apply.request" ] || fail "a refused request was left to re-fire"

# 2. A request naming a directory that is not a release is refused.
mkdir -p "$SWARM_STATE_ROOT/downloads/rubbish"
printf '%s\n' "$SWARM_STATE_ROOT/downloads/rubbish" > "$SWARM_STATE_ROOT/release-apply.request"
if sh "$SWARM_INSTALL_ROOT/current/swarm-package" apply-release >/dev/null 2>&1; then
  fail "a directory with no swarm-package was accepted"
fi
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ] || fail "rubbish changed the release"

# A Hive with something in it, so "the database survived" means something.
printf 'hive-data\n' > "$SWARM_STATE_ROOT/swarm.sqlite3"

# 3. The real thing.
printf '%s\n' "$SWARM_STATE_ROOT/downloads/2.0.0" > "$SWARM_STATE_ROOT/release-apply.request"
sh "$SWARM_INSTALL_ROOT/current/swarm-package" apply-release >/dev/null || fail "the release did not install"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "2.0.0" ] || fail "current is not 2.0.0"
grep -q 'state=installed' "$SWARM_STATE_ROOT/release-apply.status" || fail "success was not reported"
[ ! -f "$SWARM_STATE_ROOT/release-apply.request" ] || fail "the request was not consumed"
[ ! -d "$SWARM_STATE_ROOT/downloads/2.0.0" ] || fail "the installed download was not cleaned up"

# 4. The database survived, and rollback still reaches 1.0.0.
[ "$(cat "$SWARM_STATE_ROOT/swarm.sqlite3")" = "hive-data" ] || fail "the Hive database did not survive"
sh "$SWARM_INSTALL_ROOT/current/swarm-package" rollback >/dev/null || fail "rollback failed"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ] || fail "rollback did not restore 1.0.0"

# 5. An empty request is a no-op, not a crash loop.
if sh "$SWARM_INSTALL_ROOT/current/swarm-package" apply-release >/dev/null 2>&1; then
  fail "apply-release with no request succeeded"
fi

# 5a. AN INSTALL THAT FAILS SAYS WHY, IN THE BUNDLE'S OWN WORDS.
#
# This is the path a person is most likely to be standing in front of, and it
# used to write `state=failed` plus a version and nothing else — so the control
# room said "The install did not run", named no cause, and pointed at
# journalctl. The bundle always knew why; nobody caught its stderr.
mkdir -p "$SWARM_STATE_ROOT/downloads/3.0.0"
printf '3.0.0\n' > "$SWARM_STATE_ROOT/downloads/3.0.0/VERSION"
# Matching the running host's protocol keeps this on the `update` path. Without
# it apply-release correctly picks migrate-protocol, which is a different step
# and would make this test measure the wrong one.
cp "$SWARM_INSTALL_ROOT/host-current/PROTOCOL" "$SWARM_STATE_ROOT/downloads/3.0.0/PROTOCOL"
cat > "$SWARM_STATE_ROOT/downloads/3.0.0/swarm-package" <<'EOF'
#!/bin/sh
echo "swarm-package: the pre-update database backup could not be created" >&2
exit 1
EOF
chmod +x "$SWARM_STATE_ROOT/downloads/3.0.0/swarm-package"
printf '%s\n' "$SWARM_STATE_ROOT/downloads/3.0.0" > "$SWARM_STATE_ROOT/release-apply.request"
if sh "$SWARM_INSTALL_ROOT/current/swarm-package" apply-release >/dev/null 2>&1; then
  fail "a failing install reported success"
fi
grep -q '^state=failed$' "$SWARM_STATE_ROOT/release-apply.status" \
  || fail "the failed install was not reported as failed"
grep -q '^step=update$' "$SWARM_STATE_ROOT/release-apply.status" \
  || fail "the failed install did not name the step it failed at"
grep -q '^detail=.*pre-update database backup' "$SWARM_STATE_ROOT/release-apply.status" \
  || fail "the failed install did not carry the bundle's own words"

# AND IT DOES NOT CLAIM NOTHING CHANGED WITHOUT LOOKING. The card said
# "Nothing was changed and this Hive is still on X" unconditionally. Here the
# installed VERSION is read back: it is still 1.0.0, so `nothing` is a finding
# rather than a hopeful default.
grep -q '^changed=nothing$' "$SWARM_STATE_ROOT/release-apply.status" \
  || fail "the failed install did not record what it left behind"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "1.0.0" ] || fail "the failing install changed the release"

# The same field reports `partial` when the version DID move and the install
# still failed — the case where "nothing was changed" would have been a lie.
cat > "$SWARM_STATE_ROOT/downloads/3.0.0/swarm-package" <<'EOF'
#!/bin/sh
printf '3.0.0\n' > "$SWARM_INSTALL_ROOT/current/VERSION"
echo "swarm-package: the new release did not come up healthy" >&2
exit 1
EOF
chmod +x "$SWARM_STATE_ROOT/downloads/3.0.0/swarm-package"
printf '%s\n' "$SWARM_STATE_ROOT/downloads/3.0.0" > "$SWARM_STATE_ROOT/release-apply.request"
if sh "$SWARM_INSTALL_ROOT/current/swarm-package" apply-release >/dev/null 2>&1; then
  fail "a half-completed install reported success"
fi
grep -q '^changed=partial$' "$SWARM_STATE_ROOT/release-apply.status" \
  || fail "an install that moved the version still claimed nothing changed"
printf '1.0.0\n' > "$SWARM_INSTALL_ROOT/current/VERSION"
rm -rf "$SWARM_STATE_ROOT/downloads/3.0.0"

# --- the engine carries forward when it did not change ---------------------
#
# "I thought we were supposed to quietly move the worker engine to the installed
# version if there were no engine updates." An unchanged engine left
# host-current on the release it was installed from, so the card read a stale
# version beside a newer app.

make_bundle 7.0.0
printf 'engine-same\n' > "$test_root/bundle-7.0.0/WORKER_ENGINE_BUILD_ID"
printf 'engine-same\n' > "$SWARM_INSTALL_ROOT/current/WORKER_ENGINE_BUILD_ID"
host_before=$(basename "$(readlink "$SWARM_INSTALL_ROOT/host-current")")
export SWARM_RUNNING_HOST_RELEASE="$SWARM_INSTALL_ROOT/releases/$host_before"
sh "$test_root/bundle-7.0.0/swarm-package" update "$test_root/bundle-7.0.0" >/dev/null
host_after=$(basename "$(readlink "$SWARM_INSTALL_ROOT/host-current")")
[ "$host_after" = "7.0.0" ] || fail "an unchanged engine did not move to the new release ($host_before -> $host_after)"
[ -d "$SWARM_RUNNING_HOST_RELEASE" ] || fail "pruning removed the release still serving provider lifecycle hooks"
unset SWARM_RUNNING_HOST_RELEASE
# Nothing was drained or restarted to do it.
if grep -q 'drain' "$HOME/swarmctl.log" 2>/dev/null; then
  fail "carrying an unchanged engine forward must not drain the terminal host"
fi

# --- and does not, when the engine really changed ---------------------------
make_bundle 8.0.0
printf 'engine-different\n' > "$test_root/bundle-8.0.0/WORKER_ENGINE_BUILD_ID"
sh "$test_root/bundle-8.0.0/swarm-package" update "$test_root/bundle-8.0.0" >/dev/null
host_changed=$(basename "$(readlink "$SWARM_INSTALL_ROOT/host-current")")
[ "$host_changed" = "7.0.0" ] || fail "a changed engine must not be swapped under running workers ($host_changed)"

printf 'release apply smoke passed\n'

# LAST, because it moves the Hive to a new protocol: A PROTOCOL CHANGE INSTALLS FROM THE CONTROL ROOM, which it could not.
#
# apply-release hardcoded `update`, and `update` refuses a protocol change by
# design. So an operator accepting such a release in the control room got the
# refusal and no route forward — the in-app path could not reach the migration
# that exists. Choosing it here is safe rather than bold: migrate-protocol
# drains and DEFERS while any session is live, so it cannot take a worker's
# terminal out from under them.
make_bundle 9.0.0 6
mkdir -p "$SWARM_STATE_ROOT/downloads"
cp -r "$test_root/bundle-9.0.0" "$SWARM_STATE_ROOT/downloads/9.0.0"
printf '%s\n' "$SWARM_STATE_ROOT/downloads/9.0.0" > "$SWARM_STATE_ROOT/release-apply.request"
sh "$SWARM_INSTALL_ROOT/current/swarm-package" apply-release >/dev/null \
  || fail "a protocol-bumping release did not install from the control room"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "9.0.0" ] || fail "current is not 9.0.0"
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "9.0.0" ] || fail "the host was left on the old protocol"
grep -q 'protocol_migration=1' "$SWARM_STATE_ROOT/release-apply.status" \
  || fail "the status did not say a protocol migration was chosen"

# --- A PROTOCOL CHANGE ON A HIVE WITH A WORKER THAT WILL NOT STAY DOWN ------
#
# THE ACCEPTANCE THIS EXISTS FOR: "Demonstrated with an autostart worker
# actually present, because a test with none measures the case that already
# worked." The stub below never lets running_sessions reach zero, which is
# exactly what an autostart worker does — supervise_workers revives it within
# seconds of it stopping, so anything that waits for idle waits forever.
#
# Measured on the real Hive 2026-08-28: 34 workers, one autostart (Queen), and
# every reconcile pass reported "1 sessions are active" AFTER the operator had
# deliberately killed everything. The migration only completed once swarm-api
# was stopped outright.
make_bundle 11.0.0 8
mkdir -p "$SWARM_STATE_ROOT/downloads"
cp -r "$test_root/bundle-11.0.0" "$SWARM_STATE_ROOT/downloads/11.0.0"
# A worker that keeps coming back. Nothing in this test ever lowers it.
printf '3\n' > "$HOME/running-sessions"
printf '%s\n' "$SWARM_STATE_ROOT/downloads/11.0.0" > "$SWARM_STATE_ROOT/release-apply.request"
sh "$SWARM_INSTALL_ROOT/current/swarm-package" apply-release >/dev/null 2>&1 \
  || fail "a protocol change could not install while an autostart worker kept restarting"

# IT COMPLETED, with no human action and no window of idleness to wait for.
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "11.0.0" ] \
  || fail "the API was not migrated — the deferral is waiting for an idle that never comes"
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "11.0.0" ] \
  || fail "the host was left behind while the API moved"
[ "$(cat "$SWARM_INSTALL_ROOT/current/PROTOCOL")" = "$(cat "$SWARM_INSTALL_ROOT/host-current/PROTOCOL")" ] \
  || fail "the API and host protocols disagree after the install"
grep -q '^state=installed$' "$SWARM_STATE_ROOT/release-apply.status" \
  || fail "the install was not reported as installed"
grep -q '^protocol_migration=1$' "$SWARM_STATE_ROOT/release-apply.status" \
  || fail "the status did not record that this install swapped the protocol"
# And it left nothing waiting: a pending marker here would be the deadlock.
[ ! -f "$SWARM_STATE_ROOT/protocol-migration.pending" ] \
  || fail "the install left a deferred migration that nothing can complete"
[ "$(cat "$HOME/running-sessions")" = "3" ] \
  || fail "the test stopped simulating an autostart worker partway through"

printf 'protocol change with an autostart worker passed\n'

# --- THE EXPLICIT DEFERRING COMMAND STILL DEFERS ----------------------------
#
# migrate-protocol-if-idle is no longer what a release install uses, but it is
# still a supported command for someone who wants to schedule the swap rather
# than take it now. It must keep refusing to yank a live terminal.
make_bundle 12.0.0 9
cp -r "$test_root/bundle-12.0.0" "$SWARM_STATE_ROOT/downloads/12.0.0"
sh "$SWARM_INSTALL_ROOT/current/swarm-package" migrate-protocol-if-idle "$SWARM_STATE_ROOT/downloads/12.0.0" >/dev/null \
  || fail "the explicit deferring migration reported failure"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "11.0.0" ] \
  || fail "the deferring command swapped anyway while sessions were live"
[ -f "$SWARM_STATE_ROOT/protocol-migration.pending" ] \
  || fail "the deferring command left nothing to complete later"

# It completes when the sessions really do end — the case that works when
# nothing is reviving a worker underneath it.
printf '0\n' > "$HOME/running-sessions"
sh "$SWARM_INSTALL_ROOT/current/swarm-package" complete-protocol-migration-if-idle >/dev/null \
  || fail "the pending migration did not complete once sessions ended"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "12.0.0" ] \
  || fail "the pending migration did not activate"
[ "$(cat "$SWARM_INSTALL_ROOT/host-current/VERSION")" = "12.0.0" ] \
  || fail "the pending migration left the host behind"
[ ! -f "$SWARM_STATE_ROOT/protocol-migration.pending" ] \
  || fail "a completed migration is still pending"

printf 'explicit deferral still defers and still completes\n'

# THE PENDING MARKER DECIDES WHAT GETS ACTIVATED, so it is validated as a path
# inside the managed release root. Without this, whatever could write that file
# could name any directory and have it installed as the running release.
mkdir -p "$test_root/not-a-managed-release"
printf '99\n' > "$test_root/not-a-managed-release/PROTOCOL"
printf '99.0.0\n' > "$test_root/not-a-managed-release/VERSION"
printf '%s\n' "$test_root/not-a-managed-release" > "$SWARM_STATE_ROOT/protocol-migration.pending"
before_forced=$(cat "$SWARM_INSTALL_ROOT/current/VERSION")
sh "$SWARM_INSTALL_ROOT/current/swarm-package" reconcile-host-requested >/dev/null 2>&1 \
  || fail "an out-of-tree pending marker crashed the reconcile timer"
[ "$(cat "$SWARM_INSTALL_ROOT/current/VERSION")" = "$before_forced" ] \
  || fail "a pending marker outside the release root was activated"
[ ! -f "$SWARM_STATE_ROOT/protocol-migration.pending" ] \
  || fail "the rejected marker was left to be retried forever"

# A marker naming a path inside the release root that is not there is refused
# the same way, rather than half-activating nothing.
printf '%s\n' "$SWARM_INSTALL_ROOT/releases/does-not-exist" > "$SWARM_STATE_ROOT/protocol-migration.pending"
sh "$SWARM_INSTALL_ROOT/current/swarm-package" reconcile-host-requested >/dev/null 2>&1 \
  || fail "a missing pending release crashed the reconcile timer"
[ ! -f "$SWARM_STATE_ROOT/protocol-migration.pending" ] \
  || fail "a marker naming a missing release was left pending"

printf 'pending marker validation passed\n'
