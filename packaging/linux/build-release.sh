#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
version=$(sed -n 's/^version = "\([0-9][0-9.]*\)"/\1/p' "$repo_root/Cargo.toml" | head -n 1)
protocol=$(sed -n 's/^pub const PROTOCOL_VERSION: u16 = \([0-9][0-9]*\);/\1/p' "$repo_root/crates/swarm-terminal/src/ipc.rs")
[ -n "$version" ] && [ -n "$protocol" ] || { echo "could not determine package metadata" >&2; exit 1; }

output=${1:-"$repo_root/dist"}
bundle="$output/swarm-next-$version-linux-x86_64"
rm -rf -- "$bundle"
mkdir -p "$bundle/bin" "$bundle/web" "$bundle/systemd-user"

(cd "$repo_root" && cargo build --release --locked --workspace)
if [ "${SWARM_SKIP_WEB_BUILD:-0}" != "1" ]; then
  (cd "$repo_root" && "${SWARM_PNPM_BIN:-pnpm}" --dir web build)
fi
[ -f "$repo_root/web/dist/index.html" ] || { echo "compiled web assets are missing" >&2; exit 1; }
cp "$repo_root/target/release/swarm-api" "$bundle/bin/"
cp "$repo_root/target/release/swarm-terminal-host" "$bundle/bin/"
cp "$repo_root/target/release/swarmctl" "$bundle/bin/"
cp -R "$repo_root/web/dist/." "$bundle/web/"
cp "$repo_root/packaging/systemd-user/"*.in "$bundle/systemd-user/"
printf '%s\n' "$version" > "$bundle/VERSION"
printf '%s\n' "$protocol" > "$bundle/PROTOCOL"
(
  cd "$bundle"
  find bin web systemd-user -type f -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
)
tar -C "$output" -czf "$output/swarm-next-$version-linux-x86_64.tar.gz" "$(basename "$bundle")"
printf '%s\n' "$output/swarm-next-$version-linux-x86_64.tar.gz"
