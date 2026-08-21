#!/bin/sh
set -eu

repo_root=${1:?repository root is required}
source_id=$(
  cd "$repo_root"
  {
    rustc --version --verbose
    # The workspace version is a release number, not a fact about the engine.
    # `cargo tree` prints it against every workspace member, so leaving it in
    # made the fingerprint change on every release — and each release then
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
    find crates/swarm-terminal crates/swarm-terminal-host \
      -type f \( -name '*.rs' -o -name 'Cargo.toml' \) -print \
      | LC_ALL=C sort \
      | while IFS= read -r file; do
          printf 'FILE %s\n' "$file"
          sed 's/\r$//' "$file"
        done
  } | sha256sum | cut -d ' ' -f 1
)

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
