#!/bin/sh
set -eu

repo_root=${1:?repository root is required}
source_id=$(
  cd "$repo_root"
  {
    rustc --version --verbose
    cargo tree --locked --offline -p swarm-terminal-host --edges normal --prefix none \
      | sed 's# (.*)##'
    sha256sum crates/swarm-terminal/Cargo.toml crates/swarm-terminal-host/Cargo.toml
    find crates/swarm-terminal crates/swarm-terminal-host \
      -type f \( -name '*.rs' -o -name 'Cargo.toml' \) -print0 \
      | sort -z \
      | xargs -0 sha256sum
  } | sha256sum | cut -d ' ' -f 1
)

# The holder already installed from b8c68e03 has exactly this engine source and
# dependency closure. Preserve its published ID across the fingerprint-boundary
# correction so an App/API-only deployment does not request a needless restart.
# Any later terminal or holder change falls through to its new source ID.
case "$source_id" in
  7690e3a6f2e02f9c10625ee51310435d777fd9434b0b6fec2d85dc1c1979f962)
    printf '%s\n' 4c5236cf0b389e85342460e82dabee07c62c5b7171b964bc17414a93dc44bb81
    ;;
  *) printf '%s\n' "$source_id";;
esac
