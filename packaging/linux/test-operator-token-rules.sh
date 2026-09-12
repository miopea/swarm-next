#!/bin/sh
set -eu

# The operator token's length rule, which three places used to disagree about.
#
# ⚠️ THIS BRICKED A REAL INSTALL. Reported 2026-09-12 from a machine installing
# 1.9.0 off localhost: the prompt asked for "at least 12 characters",
# validate_config refused anything under 32, and write_initial_config had
# already SAVED the token the validator then rejected. The re-run skipped the
# prompt because the file existed, re-read the same token, and failed
# identically -- so the documented one-line install could not be repeated.
#
# A fourth rule sat behind those: the API accepts 16..200 for a token rotated in
# Settings, and `update`, `prepare-protocol` and `migrate-protocol` all call
# validate_config. So a legal rotation to a 20-character token would have failed
# every later update the same unrecoverable way, which nobody had hit yet.

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
package=$repo_root/packaging/linux/swarm-package
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT HUP INT TERM

# --- ONE RULE, WRITTEN ONCE ------------------------------------------------

assignments=$(grep -c '^min_operator_token=' "$package")
[ "$assignments" = "1" ] || {
  echo "the token minimum is assigned $assignments times; it must be written once" >&2
  exit 1
}

# No bare numeric token comparison may survive: that is the shape of the bug.
if grep -nE '\$\{#(token|chosen|saved)\}" -(lt|ge|gt|le) "?[0-9]+' "$package"; then
  echo "a token length is compared against a literal above; use \$min_operator_token" >&2
  exit 1
fi

# --- AND IT AGREES WITH THE API -------------------------------------------
#
# THE ASSERTION THAT WOULD HAVE CAUGHT THIS. The installer and the API must
# accept the same tokens in both directions: anything the installer writes, the
# API must take, and anything the API lets an operator rotate to must keep
# updating. Two independently reasonable numbers is exactly how this failed.

shell_min=$(sed -n 's/^min_operator_token=\([0-9]*\)$/\1/p' "$package")
shell_max=$(sed -n 's/^max_operator_token=\([0-9]*\)$/\1/p' "$package")
auth=$repo_root/crates/swarm-api/src/auth.rs
api_min=$(sed -n 's/.*token\.len() < \([0-9]*\).*/\1/p' "$auth" | head -1)
api_max=$(sed -n 's/.*token\.len() > \([0-9]*\).*/\1/p' "$auth" | head -1)

[ -n "$api_min" ] && [ -n "$api_max" ] || {
  echo "could not read the API's token bounds from $auth; this test is not measuring anything" >&2
  exit 1
}
[ "$shell_min" = "$api_min" ] || {
  echo "installer minimum $shell_min disagrees with the API's $api_min" >&2
  exit 1
}
[ "$shell_max" = "$api_max" ] || {
  echo "installer maximum $shell_max disagrees with the API's $api_max" >&2
  exit 1
}

# --- A SAVED TOKEN THAT CANNOT BE USED IS REPAIRED, NOT FATAL -------------

harness() {
  saved_token=$1
  config=$scratch/config
  rm -rf "$config"; mkdir -p "$config"
  printf 'SWARM_API_BIND=127.0.0.1:8766\nSWARM_OPERATOR_TOKEN=%s\nSWARM_WORKSPACE_ROOTS=/ws\n' \
    "$saved_token" > "$config/swarm.env"
  # No tty, which is what an unattended install has, so the repair must
  # generate rather than wait for somebody to type.
  sh -c '
    set -eu
    config_root=$1
    min_operator_token='"$shell_min"'
    max_operator_token='"$shell_max"'
    read_chosen_token() { return 1; }
    '"$(sed -n '/^repair_operator_token() {/,/^}/p;/^write_initial_config() {/,/^}/p' "$package")"'
    write_initial_config
  ' sh "$config" >/dev/null 2>&1
  sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$config/swarm.env"
}

short=$(harness "shortone1234")
[ "${#short}" -ge "$shell_min" ] || {
  echo "a saved token too short to use was left in place instead of repaired" >&2
  exit 1
}

# THE OTHER HALF, or the test above passes against code that rewrites every
# token it sees. A usable one must be left exactly alone.
usable=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
kept=$(harness "$usable")
[ "$kept" = "$usable" ] || {
  echo "a usable saved token was replaced; an install must not rewrite a working credential" >&2
  exit 1
}

# And the rest of the file survives the repair.
grep -q '^SWARM_API_BIND=127.0.0.1:8766$' "$scratch/config/swarm.env" || {
  echo "the repair lost the bind address" >&2
  exit 1
}
grep -q '^SWARM_WORKSPACE_ROOTS=/ws$' "$scratch/config/swarm.env" || {
  echo "the repair lost the workspace root" >&2
  exit 1
}

echo "operator token rules agree and an unusable saved token is recoverable"
