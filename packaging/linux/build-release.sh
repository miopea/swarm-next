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

# THE FEEDBACK CREDENTIAL THIS ARTEFACT WILL CARRY.
#
# Held in a shell variable and handed to cargo, never written to a file and
# never echoed. This script must not be run under `set -x`, which would print
# every expansion including this one.
#
# ABSENT IS A LEGITIMATE BUILD, not an error: a local package built for testing
# has no business carrying the project's credential. It is announced loudly on
# stderr, because an artefact that silently cannot file is precisely the failure
# this change exists to end.
#
# Rotation: revoke at GitHub, put the new value in the same 1Password item,
# build again. Nothing else in this tree holds a copy. Cargo re-runs the compile
# when the variable's value changes — measured, not assumed: same value 0
# recompiles, changed value 1.
#
# The REFERENCE is not a secret — it is an item name — so it lives in the tree
# where a reader can see which item a release draws from, and is overridable for
# anyone packaging from a different vault.
: "${SWARM_FEEDBACK_TOKEN_REFERENCE:=op://BFG/Swarm feedback token/credential}"
# ⚠️ A MISSING TOKEN FAILS THE BUILD. IT USED TO WARN, AND EVERY RELEASE EVER
# CUT SHIPPED WITHOUT THE CREDENTIAL.
#
# Measured 2026-09-04 against the published artefacts: v1.2.0, v1.3.1 and v1.4.1
# each contain ZERO GitHub token literals of any prefix. The feature shipped in
# 1.1.2 and has never once worked in a packaged release. A developer on 1.4.1
# reported the dialog telling her to set SWARM_GITHUB_REPOSITORY and
# SWARM_GITHUB_TOKEN herself -- which is the exact thing the bundled credential
# exists to spare her, and the operator's original complaint verbatim: "so you
# are telling me that devs need to install settings to make it work? that is
# stupid".
#
# Every branch below used to print to stderr and carry on. Nobody reads a
# successful build's stderr, so the release went out looking fine, was verified
# in code and in tests, and was broken in the only place that ships. The
# packaging guide's own rule -- verify the ARTIFACT, not your working tree --
# names this exact failure and this step did not follow it.
#
# SWARM_SKIP_BUNDLED_FEEDBACK_TOKEN=1 remains the way to build without one, and
# it is now the ONLY way: deliberate, named, and visible in the command that did
# it rather than in a line of output nobody kept.
bundled_feedback_token=""
if [ "${SWARM_SKIP_BUNDLED_FEEDBACK_TOKEN:-0}" = "1" ]; then
  echo "swarm-package: building WITHOUT a bundled feedback token (asked to skip)" >&2
elif command -v op >/dev/null 2>&1; then
  # The exit status is checked and the value asserted non-empty before it is
  # believed. Discarding either is how an empty credential becomes a downstream
  # mystery: it fails later as a permission or network problem, and the
  # investigation goes to the wrong system.
  if bundled_feedback_token=$(op read "$SWARM_FEEDBACK_TOKEN_REFERENCE"); then
    if [ -z "$bundled_feedback_token" ]; then
      echo "swarm-package: the feedback token reference resolved EMPTY." >&2
      echo "  A release without it cannot file feedback and tells every installer to" >&2
      echo "  obtain their own token. Sign in with 'eval \"\$(op-login)\"' and retry," >&2
      echo "  or pass SWARM_SKIP_BUNDLED_FEEDBACK_TOKEN=1 to build one deliberately." >&2
      exit 1
    else
      # Its LENGTH, never any part of its value.
      echo "swarm-package: bundling a feedback token (${#bundled_feedback_token} characters)" >&2
    fi
  else
    echo "swarm-package: could not read $SWARM_FEEDBACK_TOKEN_REFERENCE." >&2
    echo "  Sign in with 'eval \"\$(op-login)\"' and retry, or pass" >&2
    echo "  SWARM_SKIP_BUNDLED_FEEDBACK_TOKEN=1 to build without it deliberately." >&2
    exit 1
  fi
else
  echo "swarm-package: 'op' is not on PATH, so no feedback token can be read." >&2
  echo "  Install the 1Password CLI, or pass SWARM_SKIP_BUNDLED_FEEDBACK_TOKEN=1" >&2
  echo "  to build without it deliberately." >&2
  exit 1
fi

(cd "$repo_root" && SWARM_BUILD_VERSION="$version" SWARM_BUILD_SOURCE_REVISION="$source_revision" SWARM_WORKER_ENGINE_BUILD_ID="$worker_engine_build_id" SWARM_RELEASE_VERIFYING_KEY="$release_verifying_key" SWARM_BUNDLED_FEEDBACK_TOKEN="$bundled_feedback_token" cargo build --release --locked --workspace)

# ⚠️ THE CREDENTIAL IS IN THE BINARY, ASSERTED AGAINST THE ARTEFACT ITSELF.
#
# The env var above being set is not evidence that the token reached the
# compiled output. `bundled_feedback_destination` reads it through `option_env!`,
# a COMPILE-TIME lookup, so anything that stops the crate being rebuilt -- a
# warm cache, a changed feature set, a refactor that moves the call -- silently
# produces a binary with no credential and a build that exits 0.
#
# That is not hypothetical. v1.2.0, v1.3.1 and v1.4.1 all shipped with zero
# token literals while their source and tests were correct, and it went
# unnoticed for four releases because nothing ever looked at the artefact.
#
# THE SECRET NEVER REACHES argv. It goes to grep on stdin as a pattern file, so
# it appears in no command line, no `ps` listing and no error message. Only the
# yes/no answer is printed.
if [ -n "$bundled_feedback_token" ]; then
  api_binary="$repo_root/target/release/swarm-api"
  [ -f "$api_binary" ] || { echo "swarm-package: $api_binary is missing after the build" >&2; exit 1; }
  if printf '%s\n' "$bundled_feedback_token" | grep -qFf - "$api_binary"; then
    echo "swarm-package: the bundled feedback credential is present in swarm-api" >&2
  else
    echo "swarm-package: THE BUILD DID NOT BAKE IN THE FEEDBACK CREDENTIAL." >&2
    echo "  SWARM_BUNDLED_FEEDBACK_TOKEN was set and read, and the compiled" >&2
    echo "  swarm-api does not contain it -- option_env! is resolved at compile" >&2
    echo "  time, so a cached build can ignore a changed value. Try again with" >&2
    echo "  'cargo clean -p swarm-api', or pass SWARM_SKIP_BUNDLED_FEEDBACK_TOKEN=1" >&2
    echo "  to ship without it deliberately." >&2
    exit 1
  fi
fi
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
# What changed, for the modal shown after an update.
#
# In the BUNDLE and not the manifest, and that is forced rather than chosen: the
# manifest signature is computed over the re-serialized payload, so a field an
# older Hive does not know is dropped before it recomputes the canonical form,
# and the signature fails for the WHOLE document -- the one every Hive reads to
# learn any release exists. Bumping schema_version is no better; the check is
# exact equality. Either way the damage lands on already deployed Hives.
#
# The bundle is still covered: the manifest signs artifact_sha256, taken over
# the artifact these notes live inside. Transitively signed, no second fetch,
# and no change to the document older Hives must verify.
#
# Generated here rather than at publish time because this is where the git
# history is, and read straight back out of the bundle by swarm-package.
"$repo_root/target/release/swarm-release" notes "$repo_root" "$version" > "$bundle/NOTES" \
  || { echo "could not gather release notes" >&2; exit 1; }
(
  cd "$bundle"
  find bin web systemd-user -type f -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
  sha256sum swarm-package >> SHA256SUMS
)
tar -C "$output" -czf "$output/swarm-$version-linux-x86_64.tar.gz" "$(basename "$bundle")"
printf '%s\n' "$output/swarm-$version-linux-x86_64.tar.gz"
