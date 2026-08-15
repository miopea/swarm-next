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

printf '\n// Worker engine test edit.\n' >> "$scratch/crates/swarm-terminal-host/src/lib.rs"
holder_change=$(build_id)
[ "$holder_change" != "$baseline" ] || {
  echo "worker engine edits must produce a new build id" >&2
  exit 1
}

echo "worker engine build-id boundary passed"
