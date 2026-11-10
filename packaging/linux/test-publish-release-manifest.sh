#!/bin/sh
set -eu

# Which protocols publish-release-manifest.sh offers a release under.
#
# ⚠️ THE OFFER LIST USED TO BE TYPED BY HAND, AND A HIVE PAID FOR EVERY SLIP.
# Through 1.18.3 a Hive installs a release by itself only when the offer's
# protocol equals its running engine's (ADR 0167). 1.18.4 had to be published
# with SWARM_OFFER_PROTOCOL=19,17 typed at the command line, and leaving it off
# would have held the release on every Hive still on an engine on 17. The bundle
# already says which engines it drives, so the script now reads that instead.
#
# The signing tool is replaced by a stub that passes the document through
# unsigned: what is under test is the document, not the signature.

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT HUP INT TERM

stub=$scratch/swarm-release
cat > "$stub" <<'EOF'
#!/bin/sh
[ "$1" = sign ] || exit 2
cat
EOF
chmod +x "$stub"
: > "$scratch/key"

# bundle VERSION PROTOCOL OLDEST_ENGINE_PROTOCOL — an empty OLDEST leaves the
# file out, as a release from before app-first installs has none.
bundle() {
  name=swarm-$1-linux-x86_64
  root=$scratch/build/$name
  mkdir -p "$root"
  : > "$root/swarm-package"
  printf '%s\n' "$1" > "$root/VERSION"
  printf '%s\n' "$2" > "$root/PROTOCOL"
  [ -z "$3" ] || printf '%s\n' "$3" > "$root/OLDEST_ENGINE_PROTOCOL"
  printf 'engine-%s\n' "$1" > "$root/WORKER_ENGINE_BUILD_ID"
  tar -czf "$scratch/$name.tar.gz" -C "$scratch/build" "$name"
  printf '%s' "$scratch/$name.tar.gz"
}

publish() {
  # check=false, deliberately: several cases assert a refusal, and set -e would
  # abort the test on the failure it is asserting.
  status=0
  SWARM_RELEASE_TOOL=$stub SWARM_RELEASE_SKIP_URL_CHECK=1 \
    sh "$repo_root/packaging/linux/publish-release-manifest.sh" \
    "$scratch/key" https://example.invalid/releases/v9 "$@" \
    >"$scratch/out" 2>"$scratch/err" || status=$?
}

# version:protocol for every entry, in the order a Hive reads them.
offers() {
  python3 -I -c '
import json, sys
releases = json.load(open(sys.argv[1]))["releases"]
print(",".join(r["version"] + ":" + r["protocol"] for r in releases))
' "$scratch/out"
}

fail() {
  printf 'test-publish-release-manifest: %s\n' "$1" >&2
  printf -- '--- stdout\n' >&2; cat "$scratch/out" >&2
  printf -- '\n--- stderr\n' >&2; cat "$scratch/err" >&2
  exit 1
}

expect_offers() {
  [ "$status" = 0 ] || fail "$1: expected success, got exit $status"
  got=$(offers)
  [ "$got" = "$2" ] || fail "$1: expected offers $2, got $got"
}

expect_refusal() {
  [ "$status" = 1 ] || fail "$1: expected a refusal, got exit $status"
  [ ! -s "$scratch/out" ] || fail "$1: a refusal must not print a manifest"
  grep -q "$2" "$scratch/err" || fail "$1: the refusal must say \"$2\""
}

# --- every engine the release drives, its own protocol first, oldest last ----
#
# A Hive through 1.18.3 keeps the LAST of equal versions, so the oldest has to
# be last for a Hive on an engine on 17 to read it. 18 is listed too: the bundle
# drives it, and leaving it out would hold the release on an engine on 18.

ranged=$(bundle 9.9.1 19 17)
publish "$ranged"
expect_offers "a release that drives engines 17 to 19" "9.9.1:19,9.9.1:18,9.9.1:17"
grep -q "also offered at protocol 17 (it speaks 19)" "$scratch/err" \
  || fail "the publisher must be told a release is offered below its own protocol"

python3 -I -c '
import json, sys
releases = json.load(open(sys.argv[1]))["releases"]
same = {(r["artifact_url"], r["artifact_sha256"], r["artifact_bytes"], r["worker_engine_build_id"]) for r in releases}
sys.exit(0 if len(same) == 1 else 1)
' "$scratch/out" || fail "every offer of one release must name the same artifact"

# --- a release that drives only its own engine is offered only there -----------

publish "$(bundle 9.9.3 19 19)"
expect_offers "a release that drives only an engine on 19" "9.9.3:19"

# --- a release from before app-first installs says nothing, so only its own ----

legacy=$(bundle 9.9.2 19 "")
publish "$legacy"
expect_offers "a release without OLDEST_ENGINE_PROTOCOL" "9.9.2:19"

# --- each tarball carries its own range -----------------------------------------

publish "$ranged" "$legacy"
expect_offers "two releases" "9.9.1:19,9.9.1:18,9.9.1:17,9.9.2:19"

# --- a range that cannot be true is refused, not published ----------------------

publish "$(bundle 9.9.4 19 20)"
expect_refusal "an oldest engine newer than the release" "OLDEST_ENGINE_PROTOCOL"

publish "$(bundle 9.9.5 19 seventeen)"
expect_refusal "an oldest engine that is not a number" "OLDEST_ENGINE_PROTOCOL"

publish "$(bundle 9.9.6 nineteen 17)"
expect_refusal "a release protocol that is not a number" "declares PROTOCOL"

# --- the hand-typed list is gone, and saying so beats ignoring it ---------------
#
# A release offered under a protocol it cannot drive would be installed by a
# Hive through 1.18.3 as an ordinary update, stopping every worker without the
# hold that exists to ask first. So the old variable is refused rather than
# honoured or silently dropped.

status=0
SWARM_OFFER_PROTOCOL=19,17 SWARM_RELEASE_TOOL=$stub SWARM_RELEASE_SKIP_URL_CHECK=1 \
  sh "$repo_root/packaging/linux/publish-release-manifest.sh" \
  "$scratch/key" https://example.invalid/releases/v9 "$ranged" \
  >"$scratch/out" 2>"$scratch/err" || status=$?
expect_refusal "SWARM_OFFER_PROTOCOL set" "SWARM_OFFER_PROTOCOL"

printf 'test-publish-release-manifest: ok\n'
