#!/bin/sh
# Moves an existing swarm-next install onto the swarm identifiers.
#
# Runs detached on purpose. It stops the terminal host, and on a dogfooding
# machine the operator's own agent session is a child of that host — running
# this inline would kill the process performing the migration halfway through.
#
# Every destructive step is preceded by something that can undo it, and the
# database is copied and opened before anything is stopped. A backup nobody
# opened is a hope.
set -eu

home_dir=${HOME:?HOME is required}
old_state="$home_dir/.local/state/swarm-next"
old_config="$home_dir/.config/swarm-next"
old_install="$home_dir/.local/lib/swarm-next"
new_state="$home_dir/.local/state/swarm"
new_config="$home_dir/.config/swarm"
new_install="$home_dir/.local/lib/swarm"
units="$home_dir/.config/systemd/user"
stamp=$(date -u +%Y%m%d%H%M%S)
backup_dir="$home_dir/.local/state/swarm-migration-$stamp"
log="$backup_dir/migration.log"

mkdir -p "$backup_dir"
exec >>"$log" 2>&1
say() { printf '%s %s\n' "$(date -u +%H:%M:%S)" "$*"; }
# Overridable so this can be rehearsed against a scratch copy without touching
# the live user session. Rehearsing it with the real systemctl would stop the
# services of the machine being rehearsed on, which is the opposite of a
# rehearsal.
systemctl_bin=${SWARM_SYSTEMCTL_BIN:-systemctl}
run_systemctl() { "$systemctl_bin" --user "$@"; }
curl_bin=${SWARM_CURL_BIN:-curl}
die() { say "FAILED: $*"; exit 1; }

say "migrating swarm-next to swarm"

# --- 1. The backup, verified by opening it -----------------------------------
[ -f "$old_state/swarm-next.sqlite3" ] || die "no database at $old_state"
cp "$old_state/swarm-next.sqlite3" "$backup_dir/swarm.sqlite3"
schema=$(sqlite3 "$backup_dir/swarm.sqlite3" 'PRAGMA user_version;') || die "backup will not open"
[ -n "$schema" ] || die "backup reports no schema version"
tasks=$(sqlite3 "$backup_dir/swarm.sqlite3" 'SELECT COUNT(*) FROM tasks;') || die "backup has no task table"
say "backup verified: schema $schema, $tasks tasks, at $backup_dir/swarm.sqlite3"

# --- 2. Stop what is running, oldest dependency last -------------------------
for unit in swarm-next-host-reconcile.timer swarm-next-host-reconcile.path \
            swarm-next-development-reload.path swarm-next-api.service \
            swarm-next-terminal-host.service; do
  run_systemctl stop "$unit" >/dev/null 2>&1 || true
done
run_systemctl disable swarm-next.target >/dev/null 2>&1 || true
say "old services stopped"

# --- 3. Move state and config, leaving the old names free --------------------
# Moved rather than copied: two directories, one of them stale, is the failure
# that outlives a rename.
[ -e "$new_state" ] && die "$new_state already exists; refusing to merge"
mv "$old_state" "$new_state" || die "state directory could not be moved"
mv "$new_state/swarm-next.sqlite3" "$new_state/swarm.sqlite3" || die "database could not be renamed"
say "state moved to $new_state"

# Legacy may own this directory already. Its file is config.yaml and ours are
# *.env, so they coexist; this deliberately does not create or clear the
# directory beyond its own files.
mkdir -p "$new_config"
for name in swarm-next.env swarm-next-dev.env; do
  [ -f "$old_config/$name" ] || continue
  mv "$old_config/$name" "$new_config/$(printf '%s' "$name" | sed 's/swarm-next/swarm/')"
done
# The names are not the only thing that moved. These files carry absolute paths
# into the state directory, and renaming the file leaves those pointing at a
# directory that no longer exists — which is silent, because the API accepts a
# reload request written to a path it cannot create and then reports a build
# that nobody will ever pick up.
for env_file in "$new_config/swarm.env" "$new_config/swarm-dev.env"; do
  [ -f "$env_file" ] || continue
  sed -i "s#$old_state#$new_state#g" "$env_file"
done
say "config moved to $new_config"

# --- 4. Install the new release at the new paths ------------------------------
release=${SWARM_MIGRATION_RELEASE:?SWARM_MIGRATION_RELEASE is required}
[ -d "$release" ] || die "release directory $release is missing"
sh "$release/swarm-package" install "$release" || die "install failed"
say "new release installed"

# --- 4a. Development mode, if this machine had it ----------------------------
# Installing a release does not enable the reload watcher: that only happens
# through enable-development, so a migrated machine keeps its development
# configuration and loses the unit that acts on it. Re-enabling regenerates the
# env against the new state root and starts the watcher, which is the supported
# path rather than a second implementation of it.
dev_checkout=$(sed -n 's/^SWARM_DEV_CHECKOUT=//p' "$new_config/swarm-dev.env" 2>/dev/null || true)
if [ -n "$dev_checkout" ] && [ -d "$dev_checkout" ]; then
  sh "$release/swarm-package" enable-development "$dev_checkout" \
    || die "development mode could not be re-enabled for $dev_checkout"
  say "development reload re-enabled for $dev_checkout"
fi

# --- 5. Verify from the machine, not from the build --------------------------
ok=""
for _ in $(seq 1 60); do
  if "$curl_bin" -fsS --max-time 3 http://127.0.0.1:8766/health >/dev/null 2>&1; then ok=yes; break; fi
  sleep 2
done
[ -n "$ok" ] || die "the API did not answer after installation"
version=$("$curl_bin" -fsS http://127.0.0.1:8766/health | sed -n 's/.*"version":"\([^"]*\)".*/\1/p')
after=$(sqlite3 "$new_state/swarm.sqlite3" 'PRAGMA user_version;')
after_tasks=$(sqlite3 "$new_state/swarm.sqlite3" 'SELECT COUNT(*) FROM tasks;')
say "API answering as $version; schema $after; $after_tasks tasks"
[ "$after" = "$schema" ] || die "schema changed unexpectedly: $schema then $after"
[ "$after_tasks" = "$tasks" ] || die "task count changed: $tasks then $after_tasks"

# --- 6. Retire the old units only once the new ones are proven ---------------
for unit in swarm-next-api.service swarm-next-terminal-host.service \
            swarm-next-development-reload.path swarm-next-development-reload.service \
            swarm-next-host-reconcile.path swarm-next-host-reconcile.service \
            swarm-next-host-reconcile.timer swarm-next.target; do
  [ -e "$units/$unit" ] && mv "$units/$unit" "$backup_dir/" || true
done
rm -rf "$units/swarm-next.target.wants" 2>/dev/null || true
run_systemctl daemon-reload >/dev/null 2>&1 || true
[ -d "$old_install" ] && mv "$old_install" "$backup_dir/lib-swarm-next" || true
say "old units and install moved to $backup_dir"

say "MIGRATION COMPLETE. Every worker must be woken so it reconnects on the new MCP key."
