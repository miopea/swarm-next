#!/bin/sh
# Run the Rust/web validation commands used by CI, after dependency setup.
# Package, security-action and platform jobs still require CI; this is not a
# substitute for those jobs or real-device acceptance.
#
# ⚠️ THIS EXISTS BECAUSE TYPING THE COMMANDS FAILED TWICE IN ONE DAY, both times
# in the same shape: the check that ran was not the check that decides.
#
#   2026-09-02, afternoon  `cargo clippy --workspace --all-targets --all-features`
#                          WITHOUT `-- -D warnings`. Fourteen lints were warnings
#                          locally and errors in CI. "clippy exit=0" was reported
#                          four times against a line that was not CI's, and main
#                          was red for eight commits before a release forced a look.
#
#   2026-09-02, evening    `tsc` passed, the file was then edited, and only vitest
#                          was re-run. The web build failed on node:fs types. A
#                          clean run followed by one more edit is not a clean run.
#
# The remedy graded highest on task 01a06407 was this one: stop making the
# commands retypeable. The remedy that ticket was closed on was "remember to be
# careful", which it had itself called the weakest kind — and the second miss
# above happened an hour after that close.
#
# Keep this in step with .github/workflows. If you add a step there and not here,
# this file quietly starts lying, which is the defect it exists to prevent.
set -eu

cd "$(dirname "$0")/.."
export CARGO_INCREMENTAL=0

mode="${1:-all}"
case "$mode" in
  rust|web|all) ;;
  *) printf 'Usage: scripts/verify.sh [rust|web|all]\n' >&2; exit 2 ;;
esac
if [ "$#" -gt 1 ]; then
  printf 'Usage: scripts/verify.sh [rust|web|all]\n' >&2
  exit 2
fi

failed=""

# Every step runs even after one fails, so a single invocation reports
# everything rather than one thing at a time. Exit codes are read directly and
# never through a pipe: `cmd | tail` reports tail's status, which is how a
# failing npm run was recorded as "lint exit=0" earlier today.
step() {
  name="$1"
  shift
  printf '\n=== %s\n' "$name"
  if "$@"; then
    printf '%s: ok\n' "$name"
  else
    printf '%s: FAILED (exit %d)\n' "$name" "$?"
    failed="$failed $name"
  fi
}

if [ "$mode" = "rust" ] || [ "$mode" = "all" ]; then
  step "cargo fmt" cargo +1.97.1 fmt --all --check
  step "cargo clippy" cargo +1.97.1 clippy --workspace --all-targets --all-features -- -D warnings
  step "cargo test" cargo +1.97.1 test --workspace --all-features
  step "release terminal resize" cargo +1.97.1 test --release -p swarm-terminal resize_updates_pty_and_canonical_dimensions
fi

web_audit() (
  cd web
  sh ../scripts/audit-web.sh
)

if [ "$mode" = "web" ] || [ "$mode" = "all" ]; then
  step "web audit" web_audit
  step "web check" pnpm check
  step "web test" pnpm test
  step "dogfood test" pnpm test:dogfood
  step "web build" pnpm build
fi

if [ -n "$failed" ]; then
  printf '\nFAILED:%s\n' "$failed"
  exit 1
fi
printf '\nall checks passed\n'
