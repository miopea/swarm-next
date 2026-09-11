#!/bin/sh
set -eu

# The disk-headroom preflight in build-release.sh, exercised against a fixture.
#
# ⚠️ THIS GUARD EXISTS BECAUSE A RELEASE BUILD RAN THE VOLUME DRY. A release
# build writes several GiB into target/release, and running out partway leaves a
# half-written target tree and no artifact -- a failure that surfaces as a
# linker error thousands of lines into cargo output, hours after the operator
# could have freed the space in a second.
#
# The preflight sits immediately after the tag gate and before anything
# expensive, so this test can run the real script: it never reaches cargo.

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT HUP INT TERM

fixture=$scratch/repo
mkdir -p "$fixture/packaging/linux"
cp "$repo_root/packaging/linux/build-release.sh" "$fixture/packaging/linux/"
printf 'version = "9.9.9"\n' > "$fixture/Cargo.toml"

# build-release.sh refuses an untagged commit before it reaches the preflight,
# so the fixture has to be a real repository carrying a matching tag.
git -C "$fixture" init --quiet
git -C "$fixture" add -A
git -C "$fixture" -c user.email=test@example.invalid -c user.name=test \
  commit --quiet -m "fixture"
git -C "$fixture" tag v9.9.9

run() {
  # check=false, deliberately: every case here is about the exit status and the
  # message, and set -e would abort the test on the failures it is asserting.
  status=0
  SWARM_MIN_FREE_MIB=$1 sh "$fixture/packaging/linux/build-release.sh" \
    >"$scratch/out" 2>"$scratch/err" || status=$?
}

# --- it refuses, and the refusal is readable -------------------------------

run 999999999
[ "$status" = 1 ] || {
  echo "a threshold no host can satisfy must fail the build, got exit $status" >&2
  exit 1
}
grep -q "not enough free space to build a release" "$scratch/err" || {
  echo "the refusal must say what went wrong; stderr was:" >&2
  cat "$scratch/err" >&2
  exit 1
}
[ ! -s "$scratch/out" ] || {
  echo "the refusal must go to stderr; stdout carried:" >&2
  cat "$scratch/out" >&2
  exit 1
}

# --- it does not name directories this host does not have ------------------
#
# A tool that offers to free target/debug on a machine with no target/debug
# reads as not knowing the machine, and the reader then trusts the rest of the
# message less. The fixture has neither directory.

grep -q "Usually reclaimable" "$scratch/err" && {
  echo "nothing on this fixture is reclaimable, yet the refusal offered some:" >&2
  cat "$scratch/err" >&2
  exit 1
}

# --- and it does name them when they exist ---------------------------------

mkdir -p "$fixture/target/debug" "$fixture/dist"
: > "$fixture/target/debug/stale.o"
: > "$fixture/dist/superseded.tar.gz"
run 999999999
grep -q "Usually reclaimable" "$scratch/err" || {
  echo "target/debug and dist exist but the refusal offered nothing to free:" >&2
  cat "$scratch/err" >&2
  exit 1
}
for offered in target/debug dist; do
  grep -q "$fixture/$offered" "$scratch/err" || {
    echo "the refusal did not name $offered:" >&2
    cat "$scratch/err" >&2
    exit 1
  }
done
rm -rf "$fixture/target" "$fixture/dist"

# --- a threshold this host meets does not fire -----------------------------
#
# THE HALF THAT MAKES THE TEST MEAN SOMETHING. A guard that fires is only
# evidence if the same guard stays silent on input it must pass; without this
# case, a preflight hard-wired to refuse would pass everything above.

for threshold in 0 1; do
  run "$threshold"
  grep -q "free space" "$scratch/err" "$scratch/out" && {
    echo "SWARM_MIN_FREE_MIB=$threshold must not trip the headroom preflight:" >&2
    cat "$scratch/err" >&2
    exit 1
  }
done

# --- a threshold that is not a number is a mistake, not a zero -------------
#
# "4g" reaching an arithmetic comparison is a shell error whose message is about
# the operator's typo only if we say so. Silently reading it as no threshold
# would disable the guard exactly when someone was trying to set it.

run 4g
[ "$status" = 1 ] || {
  echo "a non-numeric SWARM_MIN_FREE_MIB must fail, got exit $status" >&2
  exit 1
}
grep -q "must be a whole number of MiB" "$scratch/err" || {
  echo "a non-numeric SWARM_MIN_FREE_MIB must say so; stderr was:" >&2
  cat "$scratch/err" >&2
  exit 1
}

echo "release disk-headroom preflight passed"
