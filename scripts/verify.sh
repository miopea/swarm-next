#!/bin/sh
# Run exactly what CI runs, so a local pass means the same thing CI's does.
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
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_INCREMENTAL=0

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

if [ "${1:-all}" = "rust" ] || [ "${1:-all}" = "all" ]; then
  step "cargo fmt" cargo fmt --all --check
  step "cargo clippy" cargo clippy --workspace --all-targets --all-features -- -D warnings
  step "cargo test" cargo test --workspace --all-features
fi

if [ "${1:-all}" = "web" ] || [ "${1:-all}" = "all" ]; then
  # `build` rather than `check`: the reload runs `tsc -b && vite build`, and a
  # type error that only the build surfaces has already failed a reload once.
  step "web build" sh -c 'cd web && npm run build'
  step "web test" sh -c 'cd web && npm test -- --run'
fi

if [ -n "$failed" ]; then
  printf '\nFAILED:%s\n' "$failed"
  exit 1
fi
printf '\nall checks passed\n'
