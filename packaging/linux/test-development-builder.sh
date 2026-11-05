#!/bin/sh
# The development builder says WHICH HALF FAILED: compiling, or writing the
# bundle. On 2026-11-05 the disk under the output filled while the compiled
# binaries were being copied in, and the reload reported "the development build
# did not compile" about a build that had. swarm-package reads exit 73 as "could
# not write its bundle" (test-package-lifecycle.sh holds that side); this holds
# the builder to giving 73 for that and only that.
#
# The real builder runs here against a scratch checkout. cargo and pnpm are
# stubs that leave the files a build leaves, and a stub cp fails the one copy a
# test asks it to, the way a full disk does.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT HUP INT TERM

checkout="$scratch/checkout"
stubs="$scratch/stubs"
mkdir -p "$checkout/packaging/linux" "$checkout/packaging/systemd-user" \
  "$checkout/crates/swarm-terminal/src" "$checkout/scripts" "$stubs"
cp "$repo_root/Cargo.toml" "$checkout/"
cp "$repo_root/crates/swarm-terminal/src/ipc.rs" "$checkout/crates/swarm-terminal/src/"
cp "$repo_root/packaging/linux/build-development-release.sh" "$repo_root/packaging/linux/swarm-package" \
  "$checkout/packaging/linux/"
cp "$repo_root/packaging/systemd-user/"*.in "$checkout/packaging/systemd-user/"
cp "$repo_root/scripts/prune-target.py" "$checkout/scripts/"
# The engine fingerprint runs the real `cargo tree`; it has its own test.
printf '#!/bin/sh\necho test-engine-build-id\n' > "$checkout/packaging/linux/worker-engine-build-id.sh"
git -C "$checkout" init -q
git -C "$checkout" config user.name "Swarm Builder Test"
git -C "$checkout" config user.email "swarm-builder-test@example.invalid"
git -C "$checkout" add Cargo.toml
git -C "$checkout" commit -qm "scratch checkout"

cat > "$stubs/cargo" <<'EOF'
#!/bin/sh
touch "$SWARM_TEST_MARKS/compiled"
if [ -n "${SWARM_TEST_CARGO_EXIT:-}" ]; then
  echo "error: could not compile \`swarm-api\` (lib) due to 1 previous error" >&2
  exit "$SWARM_TEST_CARGO_EXIT"
fi
mkdir -p target/release
for binary in swarm-api swarm-terminal-host swarmctl; do
  printf 'binary\n' > "target/release/$binary"
done
EOF
cat > "$stubs/pnpm" <<'EOF'
#!/bin/sh
mkdir -p web/dist
printf '<!doctype html>\n' > web/dist/index.html
EOF
real_cp=$(command -v cp)
cat > "$stubs/cp" <<EOF
#!/bin/sh
for argument do last=\$argument; done
for argument do
  case "\$argument" in
    *"/\${SWARM_TEST_FULL_DISK_ON:-no-file-is-full}")
      echo "cp: error writing '\$last\${SWARM_TEST_FULL_DISK_ON}': No space left on device" >&2
      exit 1;;
  esac
done
exec "$real_cp" "\$@"
EOF
chmod +x "$stubs/cargo" "$stubs/pnpm" "$stubs/cp"

marks="$scratch/marks"
build() {
  rm -rf -- "$marks"
  mkdir -p "$marks"
  build_status=0
  PATH="$stubs:$PATH" SWARM_PNPM_BIN="$stubs/pnpm" SWARM_TEST_MARKS="$marks" \
    sh "$checkout/packaging/linux/build-development-release.sh" "$1" \
    > "$scratch/stdout" 2> "$scratch/stderr" || build_status=$?
}
fail() {
  echo "$1" >&2
  sed 's/^/  builder: /' "$scratch/stderr" >&2
  exit 1
}

# 1. A build that compiles and writes its bundle returns the bundle.
build "$scratch/out"
[ "$build_status" -eq 0 ] || fail "a healthy build exited $build_status"
bundle=$(cat "$scratch/stdout")
for part in bin/swarmctl SHA256SUMS swarm-package; do
  [ -f "$bundle/$part" ] || fail "a healthy build returned a bundle without $part: $bundle"
done
! grep -q '^swarm-build:' "$scratch/stderr" || fail "a healthy build reported a failure"

# 2. THE CASE THE OPERATOR HIT: compiled, then the disk filled while copying.
SWARM_TEST_FULL_DISK_ON=swarmctl
export SWARM_TEST_FULL_DISK_ON
build "$scratch/out"
unset SWARM_TEST_FULL_DISK_ON
[ -f "$marks/compiled" ] || fail "the full-disk case never compiled, so it proves nothing"
[ "$build_status" -eq 73 ] || fail "a write failure after compiling exited $build_status, not 73"
[ "$(tail -n 1 "$scratch/stderr")" = "swarm-build: the build compiled, but writing its bundle to $scratch/out failed" ] \
  || fail "a write failure did not end by saying the build compiled and the write failed"
tail -n 2 "$scratch/stderr" | head -n 1 | grep -q 'No space left on device' \
  || fail "the writer's own error is not the line before the builder's summary"

# 3. A compile failure is not a write failure.
SWARM_TEST_CARGO_EXIT=101 build "$scratch/out"
[ "$build_status" -eq 101 ] || fail "a compile failure exited $build_status, not cargo's 101"
! grep -q '^swarm-build:' "$scratch/stderr" || fail "a compile failure was reported as a write failure"

# 4. Nor is a compiler that happens to exit 73 itself.
SWARM_TEST_CARGO_EXIT=73 build "$scratch/out"
case "$build_status" in
  0|73) fail "a compiler exiting 73 was passed on as exit $build_status";;
esac

# 5. An output that cannot even be prepared is a write failure, before compiling.
printf 'not a directory\n' > "$scratch/a-file"
build "$scratch/a-file/out"
[ "$build_status" -eq 73 ] || fail "an output that could not be prepared exited $build_status, not 73"
[ ! -f "$marks/compiled" ] || fail "the builder compiled into an output it could not prepare"
grep -q '^swarm-build: could not prepare the bundle directory in .*; nothing was compiled$' "$scratch/stderr" \
  || fail "a prepare failure did not say nothing was compiled"


# 6. The builder prunes target/ before compiling, which is what keeps the
# checkout a reload builds from under its bound.
stale="$checkout/target/release/.fingerprint/swarm-api-0123456789abcdef"
mkdir -p "$stale"
printf 'fingerprint\n' > "$stale/bin-swarm-api"
touch -d '2026-01-01 00:00:00' "$stale/bin-swarm-api" "$stale"
SWARM_TARGET_LIMIT_GIB=0.000001
export SWARM_TARGET_LIMIT_GIB
build "$scratch/out"
unset SWARM_TARGET_LIMIT_GIB
[ "$build_status" -eq 0 ] || fail "a build that pruned exited $build_status"
[ ! -e "$stale" ] || fail "the builder left a stale build unit in a target/ over its bound"
[ "$(wc -l < "$scratch/stdout")" -eq 1 ] || fail "pruning wrote to stdout, which carries only the bundle path"

# 7. A prune that fails warns and builds anyway: target/ is a cache, and a
# reload must not fail because one could not be trimmed.
cp "$checkout/scripts/prune-target.py" "$scratch/prune-target.py"
printf 'import sys\nsys.exit(3)\n' > "$checkout/scripts/prune-target.py"
build "$scratch/out"
cp "$scratch/prune-target.py" "$checkout/scripts/prune-target.py"
[ "$build_status" -eq 0 ] || fail "a failed prune failed the build with exit $build_status"
[ -f "$marks/compiled" ] || fail "a failed prune stopped the build compiling"
grep -q '^prune-target: exited 3; target/ was not pruned$' "$scratch/stderr" \
  || fail "a failed prune did not say so"
! grep -q '^swarm-build:' "$scratch/stderr" || fail "a failed prune was reported as a build failure"

echo "development builder tells a write failure from a compile failure"
echo "development builder prunes target/, and a prune that fails does not fail it"
