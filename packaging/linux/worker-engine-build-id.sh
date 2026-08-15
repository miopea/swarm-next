#!/bin/sh
set -eu

repo_root=${1:?repository root is required}
(
  cd "$repo_root"
  {
    rustc --version --verbose
    sha256sum Cargo.toml Cargo.lock
    find crates/swarm-domain crates/swarm-terminal crates/swarm-terminal-host \
      -type f \( -name '*.rs' -o -name 'Cargo.toml' \) -print0 \
      | sort -z \
      | xargs -0 sha256sum
  } | sha256sum | cut -d ' ' -f 1
)
