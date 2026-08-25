#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
base_version=$(sed -n 's/^version = "\([0-9][0-9.]*\)"/\1/p' "$repo_root/Cargo.toml" | tr -d '\r' | head -n 1)
revision=$(git -C "$repo_root" rev-parse --short=12 HEAD)
source_revision=${SWARM_SOURCE_REVISION:-$revision}
# A release is a plain semantic version and nothing else, so that two of them
# can be compared. It used to carry the revision as well, which made every
# release incomparable to every other and left an updater with nothing to go on.
#
# The tag is what declares a release, so the tag is where the number comes from.
# Refusing an untagged build is the point: a version nobody declared is a
# version nobody can reason about later.
# Captured without a pipe: a pipeline reports the last command's status, so
# piping git through sed would hide the very failure being checked for.
release_tag=$(git -C "$repo_root" describe --exact-match --tags HEAD 2>/dev/null) || {
  echo "refusing to package a release from an untagged commit: tag it first, for example 'git tag -a v$base_version -m \"Swarm $base_version\"'" >&2
  exit 1
}
version=${release_tag#v}
case "$version" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *)
    echo "release tag must be a semantic version such as v0.2.0, found '$version'" >&2
    exit 1
    ;;
esac
[ "$version" = "$base_version" ] || {
  echo "release tag $version does not match the workspace version $base_version in Cargo.toml" >&2
  exit 1
}
release_verifying_key=$(cat "$repo_root/packaging/release-verifying-key" 2>/dev/null | tr -d "\r\n")
protocol=$(sed -n 's/^pub const PROTOCOL_VERSION: u16 = \([0-9][0-9]*\);/\1/p' "$repo_root/crates/swarm-terminal/src/ipc.rs" | tr -d '\r')
worker_engine_build_id=$(sh "$repo_root/packaging/linux/worker-engine-build-id.sh" "$repo_root")
[ -n "$base_version" ] && [ -n "$revision" ] && [ -n "$source_revision" ] && [ -n "$protocol" ] && [ -n "$worker_engine_build_id" ] || { echo "could not determine package metadata" >&2; exit 1; }
case "$source_revision" in
  *[!0-9a-fA-F]*)
    echo "source revision must be exactly 12 hexadecimal characters" >&2
    exit 1
    ;;
esac
[ "${#source_revision}" -eq 12 ] || {
  echo "source revision must be exactly 12 hexadecimal characters" >&2
  exit 1
}
git -C "$repo_root" diff --quiet && git -C "$repo_root" diff --cached --quiet || {
  echo "refusing to package a dirty worktree" >&2
  exit 1
}

output=${1:-"$repo_root/dist"}
bundle="$output/swarm-$version-linux-x86_64"
rm -rf -- "$bundle"
mkdir -p "$bundle/bin" "$bundle/web" "$bundle/systemd-user"

(cd "$repo_root" && SWARM_BUILD_VERSION="$version" SWARM_BUILD_SOURCE_REVISION="$source_revision" SWARM_WORKER_ENGINE_BUILD_ID="$worker_engine_build_id" SWARM_RELEASE_VERIFYING_KEY="$release_verifying_key" cargo build --release --locked --workspace)
if [ "${SWARM_SKIP_WEB_BUILD:-0}" != "1" ]; then
  (cd "$repo_root" && VITE_SWARM_BUILD_VERSION="$version" "${SWARM_PNPM_BIN:-pnpm}" --dir web build)
fi
[ -f "$repo_root/web/dist/index.html" ] || { echo "compiled web assets are missing" >&2; exit 1; }
if [ "${SWARM_SKIP_WEB_BUILD:-0}" = "1" ]; then
  stale_web_source=$(
    find \
      "$repo_root/web/src" \
      "$repo_root/web/public" \
      "$repo_root/web/index.html" \
      "$repo_root/web/package.json" \
      "$repo_root/web/vite.config.ts" \
      "$repo_root/web/tsconfig.json" \
      -type f -newer "$repo_root/web/dist/index.html" -print -quit 2>/dev/null || true
  )
  [ -z "$stale_web_source" ] || {
    echo "refusing to package stale web assets; rebuild web/dist before using SWARM_SKIP_WEB_BUILD=1" >&2
    echo "newer source: $stale_web_source" >&2
    exit 1
  }
fi
cp "$repo_root/target/release/swarm-api" "$bundle/bin/"
cp "$repo_root/target/release/swarm-terminal-host" "$bundle/bin/"
cp "$repo_root/target/release/swarmctl" "$bundle/bin/"
cp -R "$repo_root/web/dist/." "$bundle/web/"
cp "$repo_root/packaging/systemd-user/"*.in "$bundle/systemd-user/"
cp "$repo_root/packaging/linux/swarm-package" "$bundle/"
chmod 0755 "$bundle/swarm-package"
printf '%s\n' "$version" > "$bundle/VERSION"
printf '%s\n' "$source_revision" > "$bundle/SOURCE_REVISION"
printf '%s\n' "$protocol" > "$bundle/PROTOCOL"
# Recorded so the release manifest can say whether installing this stops
# workers, at the moment of consent rather than after a reconcile timer.
printf '%s\n' "$worker_engine_build_id" > "$bundle/WORKER_ENGINE_BUILD_ID"
(
  cd "$bundle"
  find bin web systemd-user -type f -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
  sha256sum swarm-package >> SHA256SUMS
)
tar -C "$output" -czf "$output/swarm-$version-linux-x86_64.tar.gz" "$(basename "$bundle")"
printf '%s\n' "$output/swarm-$version-linux-x86_64.tar.gz"
