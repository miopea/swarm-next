#!/bin/sh
set -eu

repo_root=${1:?repository root is required}

# THIS VALUE'S ONLY JOB IS TO BE TRUSTED, SO IT MUST NOT FAIL TO A CONSTANT.
#
# swarm-package compares it against the running host_build_id and returns early
# when they match, which suppresses a restart. A wrong-but-stable id compares
# EQUAL to itself across runs, so it reads exactly like "the engine has not
# changed" — the most dangerous answer this script can give.
#
# The old form was `{ ...; } | sha256sum | cut`. Two measured failures, both
# exiting 0:
#
#   rustc absent          e3b0c44298fc — the sha256 of the EMPTY STRING, because
#                         set -e killed the producer while the pipeline's status
#                         came from `cut`. Recognisable, at least.
#   crate dir missing     d2acfc42375a — SHORT, PLAUSIBLE AND WRONG. `find` sits
#                         inside its own pipeline, so its failure does not even
#                         abort the group; the hash is simply taken over fewer
#                         files. Nothing about this value looks wrong.
#
# `set -o pipefail` is not POSIX and this is #!/bin/sh, so the fix is not to
# catch the statuses but to ASSERT THE CONTENT. The byte stream below is
# unchanged — it is redirected to a file rather than piped — so a healthy
# checkout still produces exactly the id it did before.
source_input=$(mktemp)
dependency_tree=$(mktemp)
trap 'rm -f -- "$source_input" "$dependency_tree"' EXIT HUP INT TERM

# THE CRATE LIST IS DERIVED, NEVER WRITTEN DOWN.
#
# This script used to hash exactly two directories, crates/swarm-terminal and
# crates/swarm-terminal-host, named literally. Both take swarm-domain as a path
# dependency, so the engine compiled in source this script never read, and a
# change there produced a byte-identical id -- "the engine has not changed",
# which is the one answer the header above calls the worst this script can give.
# It did that for four consecutive releases on 2026-09-02 without a tell.
#
# A hand-maintained list cannot fix that, because the failure IS the list being
# out of date, and the next path dependency someone adds would be missing from
# it in exactly the same silent way. So the list comes from cargo, which already
# knows the real dependency graph, and the guard below refuses if a crate cargo
# names cannot be located.
if ! (
  cd "$repo_root"
  # The workspace version is a release number, not a fact about the engine.
  # `cargo tree` prints it against every workspace member, so leaving it in
  # made the fingerprint change on every release -- and each release then
  # asked to restart every worker to install a terminal host whose source was
  # byte-identical. Measured between 0.4.0 and 0.5.0: no diff under
  # crates/swarm-terminal or crates/swarm-terminal-host, two different ids.
  #
  # External dependency versions are left alone. Those are facts about the
  # engine, and an upgraded one should change the fingerprint.
  # --color never matters beyond tidiness: cargo decides on colour from the
  # environment, so without it the fingerprint of identical source differs
  # between a terminal and a pipe.
  cargo tree --locked --offline --color never -p swarm-terminal-host --edges normal --prefix none \
    | sed 's#\x1b\[[0-9;]*m##g' \
    | sed 's# (.*)##' \
    | sed 's#^\( *\)\(swarm-[a-z0-9-]*\) v[0-9][0-9.]*$#\1\2 vWORKSPACE#'
) > "$dependency_tree"; then
  echo "worker engine fingerprint: cargo tree failed; refusing to emit an id" >&2
  exit 1
fi

# Every workspace crate the engine links, in cargo's own words. vWORKSPACE is
# the marker the substitution above already leaves on workspace members, so
# this reads the graph rather than a copy of it.
engine_crates=$(sed -n 's#^ *\(swarm-[a-z0-9-]*\) vWORKSPACE$#\1#p' "$dependency_tree" | LC_ALL=C sort -u)
[ -n "$engine_crates" ] || {
  echo "worker engine fingerprint: cargo named no workspace crates; refusing to emit an id" >&2
  exit 1
}

# A crate cargo names and this script cannot find is the silent-omission failure
# arriving through a different door, so it refuses rather than hashing less than
# it claims to cover.
engine_dirs=""
for crate in $engine_crates; do
  if [ ! -f "$repo_root/crates/$crate/Cargo.toml" ]; then
    echo "worker engine fingerprint: cargo names $crate but crates/$crate/Cargo.toml does not exist; refusing to emit an id" >&2
    exit 1
  fi
  engine_dirs="$engine_dirs crates/$crate"
done

if ! (
  cd "$repo_root"
  {
    rustc --version --verbose
    cat "$dependency_tree"
    # Deliberate word splitting: every element is a crates/<name> path built
    # from cargo's own crate names above.
    # shellcheck disable=SC2086
    find $engine_dirs \
      -type f \( -name '*.rs' -o -name 'Cargo.toml' \) -print \
      | LC_ALL=C sort \
      | while IFS= read -r file; do
          printf 'FILE %s\n' "$file"
          sed 's/\r$//' "$file"
        done
  }
) > "$source_input"; then
  echo "worker engine fingerprint: a required tool failed; refusing to emit an id" >&2
  exit 1
fi

# Every input must have LEFT A MARK. A missing one means the fingerprint would
# be taken over less than it claims to cover, which is the failure that has no
# other tell.
for required in \
  '^rustc ' \
  '^swarm-terminal-host ' \
  '^FILE crates/swarm-terminal/' \
  '^FILE crates/swarm-terminal-host/'
do
  grep -q "$required" "$source_input" || {
    echo "worker engine fingerprint: nothing matched $required, so an input is missing; refusing to emit an id" >&2
    exit 1
  }
done

# THE CHECK THAT MAKES THE DERIVED LIST TRUSTWORTHY. Each crate cargo named must
# have contributed a file. Deriving the list is what stops it going stale; this
# is what stops the derivation itself failing quietly -- a crate that resolves to
# a directory holding no .rs and no Cargo.toml would otherwise pass unnoticed.
for crate in $engine_crates; do
  grep -q "^FILE crates/$crate/" "$source_input" || {
    echo "worker engine fingerprint: $crate is a dependency of the engine but contributed no source; refusing to emit an id" >&2
    exit 1
  }
done

source_id=$(sha256sum < "$source_input" | cut -d ' ' -f 1)

# 0.5.0 published this same engine source under a different id, because the id
# used to move with the release number. Pinning the corrected fingerprint to
# what 0.5.0 already carries means an install on 0.5.0 sees no engine change on
# the next update, which is the truth: the terminal host source is unchanged.
#
# An install older than 0.5.0 does see one change, once. There is no way to know
# from here which engine it is actually running, and claiming no change without
# knowing would be the failure this whole fingerprint exists to prevent.
#
# Any later change to the terminal or the host falls through to its own id.
case "$source_id" in
  288c05ecd0e8f0a170f5d97fa225696311d25887e37c5d66486c009ccb0b943a)
    printf '%s\n' d12a675cd83c0431f1588e47816294d97ea0eeb6518e750f1b0b66d419d04fb7
    ;;
  *) printf '%s\n' "$source_id";;
esac
