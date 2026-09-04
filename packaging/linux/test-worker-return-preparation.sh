#!/bin/bash
set -eu

# Runs the production preparation helper without service-manager or symlink
# emulation. The full Linux package lifecycle harness remains a separate gate.
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
test_root=$(mktemp -d)
trap 'case "$test_root" in /tmp/*) rm -rf -- "$test_root";; esac' EXIT
state_root="$test_root/state"
config_root="$test_root/config"
health_url=http://127.0.0.1:8766/health
mkdir -p "$state_root" "$config_root"
printf 'SWARM_OPERATOR_TOKEN=fixture-token-not-a-credential\n' > "$config_root/swarm.env"

# Load the real helper, not a test copy. Authentication is replaced by a strict
# local spy so no network or actual credential is used.
source <(sed -n '/^record_loaded_worker_returns() {$/,/^}$/p' "$repo_root/packaging/linux/swarm-package")
calls=0
fail=0
authenticated_curl() {
  calls=$((calls + 1))
  [ -f "$1" ]
  grep -q '^header = "Authorization: Bearer fixture-token-not-a-credential"$' "$1"
  shift
  case " $* " in *' --request POST '*) :;; *) return 90;; esac
  case " $* " in *' --max-time 10 '*) :;; *) return 91;; esac
  case " $* " in *'/api/v1/runtime/terminal-host/prepare-return '*) :;; *) return 92;; esac
  return "$fail"
}

running=0
record_loaded_worker_returns
[ "$calls" -eq 0 ]
running=3
record_loaded_worker_returns
[ "$calls" -eq 1 ]
[ -z "$return_auth" ]
[ -z "$(find "$state_root" -name '.return-auth-*' -print)" ]
fail=22
if record_loaded_worker_returns; then
  echo 'failed recording was accepted' >&2
  exit 1
fi
[ "$calls" -eq 2 ]
[ -z "$return_auth" ]
[ -z "$(find "$state_root" -name '.return-auth-*' -print)" ]
rm "$config_root/swarm.env"
if record_loaded_worker_returns; then
  echo 'missing authentication was accepted' >&2
  exit 1
fi
[ "$calls" -eq 2 ]
printf 'Worker return preparation checks passed\n'
