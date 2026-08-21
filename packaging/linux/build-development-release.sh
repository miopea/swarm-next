#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
base_version=$(sed -n 's/^version = "\([0-9][0-9.]*\)"/\1/p' "$repo_root/Cargo.toml" | tr -d '\r' | head -n 1)
revision=$(git -C "$repo_root" rev-parse --short=12 HEAD)
timestamp=$(date -u +%Y%m%d%H%M%S)
version="$base_version-dev-$revision-$timestamp-$$"
release_verifying_key=$(cat "$repo_root/packaging/release-verifying-key" 2>/dev/null | tr -d "\r\n")
protocol=$(sed -n 's/^pub const PROTOCOL_VERSION: u16 = \([0-9][0-9]*\);/\1/p' "$repo_root/crates/swarm-terminal/src/ipc.rs" | tr -d '\r')
worker_engine_build_id=$(sh "$repo_root/packaging/linux/worker-engine-build-id.sh" "$repo_root")
[ -n "$base_version" ] && [ -n "$revision" ] && [ -n "$protocol" ] && [ -n "$worker_engine_build_id" ] || { echo "could not determine development package metadata" >&2; exit 1; }

output=${1:?development output directory is required}
bundle="$output/swarm-$version-linux-x86_64"
rm -rf -- "$bundle"
mkdir -p "$bundle/bin" "$bundle/web" "$bundle/systemd-user"

(cd "$repo_root" && SWARM_BUILD_VERSION="$version" SWARM_BUILD_SOURCE_REVISION="$revision" SWARM_WORKER_ENGINE_BUILD_ID="$worker_engine_build_id" SWARM_RELEASE_VERIFYING_KEY="$release_verifying_key" cargo build --release --locked --workspace) >&2
(cd "$repo_root" && "${SWARM_PNPM_BIN:-pnpm}" --dir web build) >&2
[ -f "$repo_root/web/dist/index.html" ] || { echo "compiled web assets are missing" >&2; exit 1; }
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
# Recorded so the release manifest can say whether installing this stops
# workers, at the moment of consent rather than after a reconcile timer.
printf '%s\n' "$worker_engine_build_id" > "$bundle/WORKER_ENGINE_BUILD_ID"
(
  cd "$bundle"
  find bin web systemd-user -type f -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
  sha256sum swarm-package >> SHA256SUMS
)
printf '%s\n' "$bundle"
