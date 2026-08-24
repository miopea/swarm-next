#!/bin/sh
# Builds and signs the manifest that Hives read to learn a release exists.
#
# The manifest states what is offered NOW: a release is withdrawn by ceasing to
# list it, so this rewrites the whole document from the tarballs named on the
# command line rather than appending to what is already published.
#
# Usage:
#   publish-release-manifest.sh SIGNING_KEY BASE_URL TARBALL [TARBALL...] > releases.json
#
# SIGNING_KEY is the private key from `swarm-release keygen`, which lives in
# 1Password and should be written to a temporary file only for the duration of
# this command. BASE_URL is where the tarballs are served from.
set -eu

die() { printf 'publish-release-manifest: %s\n' "$1" >&2; exit 1; }

[ "$#" -ge 3 ] || die "usage: publish-release-manifest.sh SIGNING_KEY BASE_URL TARBALL [TARBALL...]"
signing_key=$1
base_url=$2
shift 2

[ -f "$signing_key" ] || die "signing key not found"
case "$base_url" in
  https://*) :;;
  *) die "the base URL must be https; a signed document that downgrades its own transport is still worth refusing";;
esac

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
release_tool=${SWARM_RELEASE_TOOL:-"$repo_root/target/release/swarm-release"}
[ -x "$release_tool" ] || die "swarm-release is not built; run: cargo build --release -p swarm-release-tool"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

offers=""
for tarball in "$@"; do
  [ -f "$tarball" ] || die "$tarball is not there"
  name=$(basename "$tarball")
  digest=$(sha256sum "$tarball" | cut -d' ' -f1)
  bytes=$(wc -c < "$tarball" | tr -d ' ')

  # Read the release's own declarations rather than parsing its filename: the
  # bundle is the authority on what it is.
  opened="$work/opened"
  rm -rf "$opened"; mkdir -p "$opened"
  tar -xzf "$tarball" -C "$opened"
  bundle=$(find "$opened" -maxdepth 2 -name swarm-package -type f -exec dirname {} \; | head -n 1)
  [ -n "$bundle" ] || die "$name is not a Swarm release"
  version=$(tr -d '\r\n' < "$bundle/VERSION")
  protocol=$(tr -d '\r\n' < "$bundle/PROTOCOL")
  [ -f "$bundle/WORKER_ENGINE_BUILD_ID" ] || die "$name predates the recorded engine build id and cannot be offered"
  engine=$(tr -d '\r\n' < "$bundle/WORKER_ENGINE_BUILD_ID")
  case "$version" in
    *-dev-*) die "$name is a development build; only releases are offered";;
  esac

  artifact_url="${base_url%/}/$name"

  # Signing is what makes an offer real, so the URL is proved BEFORE it is
  # signed. 0.8.7 shipped a manifest whose base_url was missing its /vX.Y.Z/
  # segment: every Hive saw the release, fetched a 404, and answered its own
  # Download with 502. A manifest that names an artifact nobody can fetch stops
  # every upgrade at once, and nothing in this pipeline was checking.
  if [ -z "${SWARM_RELEASE_SKIP_URL_CHECK:-}" ]; then
    status=$(curl -sSL -o /dev/null -w '%{http_code}' --retry 2 --max-time 60 -I "$artifact_url" 2>/dev/null) \
      || die "could not reach $artifact_url to verify it; publish the GitHub release first"
    [ "$status" = "200" ] \
      || die "$artifact_url answers $status, not 200 — the manifest would offer an artifact no Hive can download"
  fi

  offer=$(printf '{"version":"%s","protocol":"%s","artifact_url":"%s","artifact_sha256":"%s","artifact_bytes":%s,"worker_engine_build_id":"%s","notes_url":null}' \
    "$version" "$protocol" "$artifact_url" "$digest" "$bytes" "$engine")
  if [ -z "$offers" ]; then offers=$offer; else offers="$offers,$offer"; fi
done

printf '{"schema_version":1,"issued_at":%s,"releases":[%s]}' "$(date -u +%s)" "$offers" \
  | "$release_tool" sign "$signing_key"
