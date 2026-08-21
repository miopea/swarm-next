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

printf '\n// Task-domain-only test edit.\n' >> "$scratch/crates/swarm-domain/src/lib.rs"
domain_only=$(build_id)
[ "$domain_only" = "$baseline" ] || {
  echo "task-domain-only edits must not request a worker engine restart" >&2
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

echo "worker engine build-id boundary passed"
