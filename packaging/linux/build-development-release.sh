#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
base_version=$(sed -n 's/^version = "\([0-9][0-9.]*\)"/\1/p' "$repo_root/Cargo.toml" | tr -d '\r' | head -n 1)
revision=$(git -C "$repo_root" rev-parse --short=12 HEAD)
timestamp=$(date -u +%Y%m%d%H%M%S)
version="$base_version-dev-$revision-$timestamp-$$"
release_verifying_key=$(cat "$repo_root/packaging/release-verifying-key" 2>/dev/null | tr -d "\r\n")
protocol=$(sed -n 's/^pub const PROTOCOL_VERSION: u16 = \([0-9][0-9]*\);/\1/p' "$repo_root/crates/swarm-terminal/src/ipc.rs" | tr -d '\r')
oldest_engine_protocol=$(sed -n 's/^pub const OLDEST_ENGINE_PROTOCOL: u16 = \([0-9][0-9]*\);/\1/p' "$repo_root/crates/swarm-terminal/src/ipc.rs" | tr -d '\r')
worker_engine_build_id=$(sh "$repo_root/packaging/linux/worker-engine-build-id.sh" "$repo_root")
[ -n "$base_version" ] && [ -n "$revision" ] && [ -n "$protocol" ] && [ -n "$oldest_engine_protocol" ] && [ -n "$worker_engine_build_id" ] || { echo "could not determine development package metadata" >&2; exit 1; }

output=${1:?development output directory is required}
bundle="$output/swarm-$version-linux-x86_64"

# WRITING THE BUNDLE IS NOT COMPILING IT. On 2026-11-05 the disk under the
# output filled while the compiled binaries were being copied in, and the
# reload said "the development build did not compile" about a build that had.
# A failure while preparing or writing the bundle exits 73 (EX_CANTCREAT) and
# says which; nothing else exits 73, so swarm-package can tell the two apart.
# The failing command's own error, such as the disk being full, is the line
# just before the "swarm-build:" summary.
cannot_write=73
phase=prepare
finish() {
  status=$?
  [ "$status" -ne 0 ] || return 0
  case "$phase" in
    prepare)
      echo "swarm-build: could not prepare the bundle directory in $output; nothing was compiled" >&2
      exit "$cannot_write";;
    write)
      echo "swarm-build: the build compiled, but writing its bundle to $output failed" >&2
      exit "$cannot_write";;
    *) [ "$status" -ne "$cannot_write" ] || exit 1;;
  esac
}
trap finish EXIT

rm -rf -- "$bundle"
mkdir -p "$bundle/bin" "$bundle/web" "$bundle/systemd-user"

# Keep target/ under its bound before adding a build to it (docs/developing.md).
# A reload must not fail because a cache could not be trimmed, and its output
# goes to stderr because stdout is the bundle path.
# It runs where cargo runs, so a relative CARGO_TARGET_DIR names the same place.
(cd "$repo_root" && python3 scripts/prune-target.py) >&2 \
  || echo "prune-target: exited $?; target/ was not pruned" >&2

phase=compile
(cd "$repo_root" && SWARM_BUILD_VERSION="$version" SWARM_BUILD_SOURCE_REVISION="$revision" SWARM_WORKER_ENGINE_BUILD_ID="$worker_engine_build_id" SWARM_RELEASE_VERIFYING_KEY="$release_verifying_key" cargo build --release --locked --workspace) >&2
(cd "$repo_root" && VITE_SWARM_BUILD_VERSION="$version" "${SWARM_PNPM_BIN:-pnpm}" --dir web build) >&2
[ -f "$repo_root/web/dist/index.html" ] || { echo "compiled web assets are missing" >&2; exit 1; }
phase='write'
cp "$repo_root/target/release/swarm-api" "$bundle/bin/"
cp "$repo_root/target/release/swarm-terminal-host" "$bundle/bin/"
cp "$repo_root/target/release/swarmctl" "$bundle/bin/"
cp -R "$repo_root/web/dist/." "$bundle/web/"
cp "$repo_root/packaging/systemd-user/"*.in "$bundle/systemd-user/"
cp "$repo_root/packaging/linux/swarm-package" "$bundle/"
chmod 0755 "$bundle/swarm-package"
printf '%s\n' "$version" > "$bundle/VERSION"
printf '%s\n' "$revision" > "$bundle/SOURCE_REVISION"
printf '%s\n' "$protocol" > "$bundle/PROTOCOL"
# The oldest engine this API drives. swarm-package installs this app alone in
# front of such an engine, and leaves the engine for the card (ADR 0157).
printf '%s\n' "$oldest_engine_protocol" > "$bundle/OLDEST_ENGINE_PROTOCOL"
# Recorded so the release manifest can say whether installing this stops
# workers, at the moment of consent rather than after a reconcile timer.
printf '%s\n' "$worker_engine_build_id" > "$bundle/WORKER_ENGINE_BUILD_ID"
(
  cd "$bundle"
  find bin web systemd-user -type f -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
  sha256sum swarm-package >> SHA256SUMS
)
printf '%s\n' "$bundle"
