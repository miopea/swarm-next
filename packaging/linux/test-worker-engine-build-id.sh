#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
scratch=$(mktemp -d)
second_checkout=$(mktemp -d)
trap 'rm -rf -- "$scratch" "$second_checkout"' EXIT HUP INT TERM

cp "$repo_root/Cargo.toml" "$repo_root/Cargo.lock" "$scratch/"
cp -R "$repo_root/crates" "$scratch/"
mkdir -p "$scratch/packaging/linux"
cp "$repo_root/packaging/linux/worker-engine-build-id.sh" "$scratch/packaging/linux/"

build_id() {
  sh "$scratch/packaging/linux/worker-engine-build-id.sh" "$scratch"
}

baseline=$(build_id)
cp -R "$scratch/." "$second_checkout/"
second_location=$(sh "$second_checkout/packaging/linux/worker-engine-build-id.sh" "$second_checkout")
[ "$second_location" = "$baseline" ] || {
  echo "checkout location must not change the worker engine build id" >&2
  exit 1
}

sed 's/$/\r/' "$scratch/crates/swarm-terminal-host/src/main.rs" > "$scratch/host-main.crlf"
mv "$scratch/host-main.crlf" "$scratch/crates/swarm-terminal-host/src/main.rs"
crlf_checkout=$(build_id)
[ "$crlf_checkout" = "$baseline" ] || {
  echo "checkout line endings must not change the worker engine build id" >&2
  exit 1
}

# THIS ASSERTION USED TO RUN THE OTHER WAY, and reversing it was the whole
# point of the change. It read "task-domain-only edits must not request a worker
# engine restart" and required a swarm-domain edit to leave the id alone.
#
# That was defensible when it was written -- the terminal host does not use
# TaskState, so a task-domain edit really is harmless to it -- but the property
# it asserted is a fact about the current contents of swarm-domain, not about
# the engine, and nothing fires at the moment it stops being true.
#
# It has already stopped being true once. Measured 2026-09-02 by building
# swarm-terminal-host --release twice per row, deterministically (an identical
# source rebuild is byte-identical, so a difference means something):
#
#   swarm-domain tasks.rs, semantics changed      binary IDENTICAL
#   swarm-domain ProviderKind::as_str changed     binary IDENTICAL
#   swarm-domain WorkerSessionId::new changed     binary DIFFERS
#
# The host links WorkerSessionId::new, WorkerId::new, PresenceDeviceId and
# FederationStewardTakeoverLeaseId -- all of them in swarm-domain. So a
# swarm-domain edit can and does change the compiled engine, and the old
# assertion forbade the fingerprint from noticing.
#
# The cost is deliberate and is the cheaper side of the trade: the fingerprint
# now moves for swarm-domain edits that do NOT change the binary, so some
# restarts are unnecessary. An engine reconcile defers while any session is
# mid-turn and surfaces on the worker engine card, so an unnecessary restart
# waits for idle -- while a missed one runs the wrong engine silently.
printf '\n// Linked-crate test edit.\n' >> "$scratch/crates/swarm-domain/src/lib.rs"
domain_edit=$(build_id)
[ "$domain_edit" != "$baseline" ] || {
  echo "an edit to a workspace crate the engine links must move the build id" >&2
  exit 1
}
# Put it back; every assertion below compares against the baseline.
cp "$repo_root/crates/swarm-domain/src/lib.rs" "$scratch/crates/swarm-domain/src/lib.rs"
[ "$(build_id)" = "$baseline" ] || {
  echo "restoring the linked-crate edit did not restore the build id" >&2
  exit 1
}

# A release number is not a fact about the engine. This was the whole cause of
# every app update asking to restart every worker: cargo tree prints the
# workspace version against each member, so bumping it moved the fingerprint
# while the terminal host source stayed byte-identical.
cp "$scratch/Cargo.toml" "$scratch/Cargo.toml.bak"
cp "$scratch/Cargo.lock" "$scratch/Cargo.lock.bak"
sed "s/^version = \"[0-9][0-9.]*\"/version = \"99.99.99\"/" "$scratch/Cargo.toml.bak" > "$scratch/Cargo.toml"
# Only the workspace members move, which is what a release bump actually does.
# Rewriting every package would just corrupt the lockfile.
awk '
  /^name = "swarm-/ { workspace = 1 }
  /^name = / && !/^name = "swarm-/ { workspace = 0 }
  workspace && /^version = / { print "version = \"99.99.99\""; next }
  { print }
' "$scratch/Cargo.lock.bak" > "$scratch/Cargo.lock"
release_bump=$(build_id)
mv "$scratch/Cargo.toml.bak" "$scratch/Cargo.toml"
mv "$scratch/Cargo.lock.bak" "$scratch/Cargo.lock"
[ "$release_bump" = "$baseline" ] || {
  echo "a release number change must not request a worker engine restart" >&2
  exit 1
}

# The fingerprint must not depend on whether stdout is a terminal, or the same
# source produces different ids in CI and on a developer's machine.
piped=$(build_id | cat)
[ "$piped" = "$baseline" ] || {
  echo "the worker engine build id must not depend on terminal detection" >&2
  exit 1
}

printf '\n// Worker engine test edit.\n' >> "$scratch/crates/swarm-terminal-host/src/lib.rs"
holder_change=$(build_id)
[ "$holder_change" != "$baseline" ] || {
  echo "worker engine edits must produce a new build id" >&2
  exit 1
}

# A BROKEN INPUT MUST REFUSE, NOT ANSWER. Both shapes were measured on the real
# script before this guard existed, and both exited 0:
#
#   rustc absent        e3b0c44298fc — the sha256 of the empty string
#   crate dir missing   d2acfc42375a — short, plausible, wrong, and stable
#
# The second is the one that matters. It has no tell, and because swarm-package
# returns early when this id matches the running host's, a stable wrong value
# reads as "the engine has not changed" and suppresses a restart forever.
#
# These run LAST because they damage the scratch checkout.
if PATH=/usr/bin:/bin sh "$scratch/packaging/linux/worker-engine-build-id.sh" "$scratch" >/dev/null 2>&1; then
  echo "a missing toolchain still produced a build id" >&2
  exit 1
fi

# The crate list is derived from cargo rather than written down, so the failure
# to guard against is no longer "the list is stale" but "the derivation resolved
# to nothing". A crate cargo names whose directory holds no source must refuse.
rm -f "$scratch/crates/swarm-domain/src/"*.rs "$scratch/crates/swarm-domain/Cargo.toml"
if build_id >/dev/null 2>&1; then
  echo "a dependency contributing no source still produced a build id" >&2
  exit 1
fi
cp -R "$repo_root/crates/swarm-domain" "$scratch/crates/"

rm -rf "$scratch/crates/swarm-terminal-host"
if build_id >/dev/null 2>&1; then
  echo "a missing engine crate still produced a build id" >&2
  exit 1
fi

echo "worker engine build-id boundary passed"
