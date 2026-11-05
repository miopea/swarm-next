#!/usr/bin/env python3
"""Keeps cargo's target/ under a size bound by evicting stale build units.

Usage: prune-target.py [--target DIR] [--limit-gib N] [--dry-run]

WHY THIS EXISTS. Cargo writes a new hashed artifact whenever a crate's build
identity changes (a version bump, a dependency update, a different feature
set or toolchain) and never deletes the old one. On 2026-11-05 the two
swarm-next checkouts on the development machine held 90 GB of build output.
One of them held 127 builds of swarm-api's test binary at about 600 MB each,
on a disk that was 86% full. target/ is a pure cache: Swarm runs from its
installed releases, never from target/.

WHAT IT EVICTS. A cargo build unit is everything named with one 16-hex-digit
hash: its .fingerprint/ entry, its deps/ files, its build/ directory and any
examples/ output. A unit is evicted whole, fingerprint first, so cargo finds no
record of it and simply builds it again. Each incremental/ directory is a unit
of its own, because its name carries a different hash.

WHICH ONES. Least recently used first, judged by the newest access or
modification time of any of the unit's files. Cargo reads every unit's
fingerprint on every build, including the ones it does not recompile, so a unit
still in use keeps a recent access time. Linux records an access at most once a
day by default (relatime), so an access time can lag a day behind the last use.
Units that look used within the last 48 hours are never evicted, which keeps
everything a build read in the last day. On a filesystem mounted noatime only
modification times count: still safe, but a dependency built long ago and used
every day can be evicted and rebuilt.

WHEN. Only while target/ is over the bound, and then down to three quarters of
it, so the next build has room before this has to run again.

NEVER UNDER A RUNNING BUILD. Cargo holds .cargo-lock and .cargo-build-lock in
a profile directory while it compiles, and waits on them when another process
holds them. Each profile directory is pruned only while this holds all of
cargo's locks there, taken without waiting. A profile whose lock is held is
left alone and reported. Cargo releases its locks once it starts running test
binaries; what those runs use was read by cargo moments before, so the 48-hour
window keeps it.
"""

from __future__ import annotations

import argparse
import fcntl
import os
import re
import shutil
import stat
import sys
import time
from dataclasses import dataclass, field

GIB = 1024**3
DEFAULT_LIMIT_GIB = 20.0
# Prune down to this fraction of the bound, so a build that follows has room.
LOW_WATER = 0.75
# A unit read within the last day can show an access time up to a day old
# under relatime, so twice that keeps every unit a build read in the last day.
PROTECT_SECONDS = 48 * 60 * 60
LOCK_NAMES = (".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock")
HASHED = re.compile(r"-([0-9a-f]{16})(?:\.|$)")
# Directories in a profile whose entries are named with the unit's hash.
HASHED_DIRS = (".fingerprint", "deps", "build", "examples")
# One line per run, in target/ itself, so the bound can be checked over weeks
# rather than at one moment. Trimmed to the newest LOG_RUNS once it doubles.
LOG_NAME = "prune-target.log"
LOG_RUNS = 1000


@dataclass
class Unit:
    """Everything one build unit left in a profile directory."""

    paths: list[str] = field(default_factory=list)
    last_used: float = 0.0
    # Bytes that removing this unit frees. A file with another hard link
    # elsewhere, such as a binary cargo copied up beside deps/, frees nothing
    # until that link goes too, so it is not counted.
    freed: int = 0


def disk_usage(path: str) -> int:
    """Bytes allocated under `path`, counting each inode once."""
    seen: set[tuple[int, int]] = set()
    total = 0
    for root, dirs, files in os.walk(path):
        for name in dirs + files:
            try:
                st = os.lstat(os.path.join(root, name))
            except FileNotFoundError:
                continue
            key = (st.st_dev, st.st_ino)
            if key in seen:
                continue
            seen.add(key)
            total += st.st_blocks * 512
    return total


def describe(path: str, unit: Unit) -> None:
    """Adds `path`, and everything under it, to the unit's recency and size."""
    entries = [path]
    if os.path.isdir(path) and not os.path.islink(path):
        for root, dirs, files in os.walk(path):
            entries.extend(os.path.join(root, name) for name in dirs + files)
    for entry in entries:
        try:
            st = os.lstat(entry)
        except FileNotFoundError:
            continue
        if stat.S_ISDIR(st.st_mode):
            # Listing a directory, as this does, sets its access time, and
            # cargo opens fingerprint files by name without listing anything.
            # A directory's access time says nothing about a build.
            unit.last_used = max(unit.last_used, st.st_mtime)
            unit.freed += st.st_blocks * 512
            continue
        unit.last_used = max(unit.last_used, st.st_atime, st.st_mtime)
        if st.st_nlink <= 1:
            unit.freed += st.st_blocks * 512
    unit.paths.append(path)


def profile_units(profile: str) -> list[Unit]:
    """The build units in one profile directory, such as target/debug."""
    by_hash: dict[str, Unit] = {}
    for directory in HASHED_DIRS:
        root = os.path.join(profile, directory)
        if not os.path.isdir(root):
            continue
        for name in os.listdir(root):
            match = HASHED.search(name)
            if match:
                describe(os.path.join(root, name), by_hash.setdefault(match.group(1), Unit()))
    units = list(by_hash.values())
    incremental = os.path.join(profile, "incremental")
    if os.path.isdir(incremental):
        for name in os.listdir(incremental):
            unit = Unit()
            describe(os.path.join(incremental, name), unit)
            units.append(unit)
    return units


def profiles(target: str) -> list[str]:
    """Profile directories: target/debug, target/release, target/<triple>/debug."""
    found = []
    for first in sorted(os.listdir(target)):
        path = os.path.join(target, first)
        if not os.path.isdir(path) or os.path.islink(path):
            continue
        if os.path.isdir(os.path.join(path, ".fingerprint")):
            found.append(path)
            continue
        for second in sorted(os.listdir(path)):
            nested = os.path.join(path, second)
            # A link could lead out of target/, so only what is really here.
            if not os.path.islink(nested) and os.path.isdir(os.path.join(nested, ".fingerprint")):
                found.append(nested)
    return found


def lock_profile(profile: str) -> list[int] | None:
    """Takes every cargo lock in `profile` without waiting, or none of them.

    A lock file that does not exist yet is created, the way cargo would, so a
    build that starts while this runs waits for it rather than racing it.
    """
    held = []
    for name in LOCK_NAMES:
        fd = os.open(os.path.join(profile, name), os.O_RDWR | os.O_CREAT, 0o644)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            os.close(fd)
            unlock(held)
            return None
        held.append(fd)
    return held


def unlock(held: list[int]) -> None:
    for fd in held:
        fcntl.flock(fd, fcntl.LOCK_UN)
        os.close(fd)


def remove(path: str) -> None:
    if os.path.isdir(path) and not os.path.islink(path):
        shutil.rmtree(path, ignore_errors=True)
    else:
        try:
            os.unlink(path)
        except FileNotFoundError:
            pass


def evict(unit: Unit) -> None:
    # The fingerprint goes first. If this is interrupted partway, cargo finds
    # no record of the unit and builds it again, instead of trusting a record
    # whose outputs are half gone.
    for path in sorted(unit.paths, key=lambda path: f"{os.sep}.fingerprint{os.sep}" not in path):
        remove(path)


def gib(size: float) -> str:
    return f"{size / GIB:.1f} GiB"


def record(target: str, line: str) -> None:
    """Appends one run to the target's log, which is kept bounded."""
    path = os.path.join(target, LOG_NAME)
    try:
        with open(path, "a", encoding="utf-8") as handle:
            handle.write(line + "\n")
        with open(path, encoding="utf-8") as handle:
            lines = handle.readlines()
        if len(lines) > 2 * LOG_RUNS:
            temporary = f"{path}.{os.getpid()}"
            with open(temporary, "w", encoding="utf-8") as handle:
                handle.writelines(lines[-LOG_RUNS:])
            os.replace(temporary, path)
    except OSError as error:
        # The log is evidence about the bound, not part of keeping it.
        print(f"prune-target: could not record this run in {path}: {error}")


def run_line(now: float, before: int, after: int, limit: int, evicted: int, busy: int) -> str:
    stamp = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(now))
    mib = 1024**2
    return (
        f"{stamp} before_mib={before // mib} after_mib={after // mib} "
        f"bound_mib={limit // mib} evicted={evicted} busy_profiles={busy}"
    )


def prune(target: str, limit: int, now: float, dry_run: bool = False) -> int:
    """Prunes `target` to under `limit` bytes. Returns the bytes it freed."""
    if not os.path.isdir(target):
        print(f"prune-target: {target} does not exist; nothing to prune")
        return 0
    total = disk_usage(target)
    if total <= limit:
        print(f"prune-target: {target} is {gib(total)}, under the {gib(limit)} bound")
        if not dry_run:
            record(target, run_line(now, total, total, limit, 0, 0))
        return 0
    goal = int(limit * LOW_WATER)
    print(f"prune-target: {target} is {gib(total)}, over the {gib(limit)} bound; pruning to {gib(goal)}")
    locks: list[list[int]] = []
    candidates: list[Unit] = []
    busy = 0
    try:
        for profile in profiles(target):
            held = lock_profile(profile)
            if held is None:
                print(f"prune-target: a build is running in {profile}; left alone")
                busy += 1
                continue
            locks.append(held)
            candidates.extend(
                unit for unit in profile_units(profile) if now - unit.last_used >= PROTECT_SECONDS
            )
        candidates.sort(key=lambda unit: unit.last_used)
        freed = 0
        evicted = 0
        for unit in candidates:
            if total - freed <= goal:
                break
            if not dry_run:
                evict(unit)
            freed += unit.freed
            evicted += 1
    finally:
        for held in locks:
            unlock(held)
    verb = "would evict" if dry_run else "evicted"
    print(f"prune-target: {verb} {evicted} unused build units, {gib(freed)}")
    if not dry_run:
        remaining = disk_usage(target)
        print(f"prune-target: {target} is now {gib(remaining)}")
        if remaining > limit:
            print(
                f"prune-target: still over the bound: what builds used in the last "
                f"{PROTECT_SECONDS // 3600} hours, or a running build's directory, was kept"
            )
        record(target, run_line(now, total, remaining, limit, evicted, busy))
    return freed


def main(argv: list[str]) -> int:
    repo = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--target",
        default=os.environ.get("CARGO_TARGET_DIR") or os.path.join(repo, "target"),
        help="cargo target directory (default: $CARGO_TARGET_DIR, else the repository's target/)",
    )
    parser.add_argument(
        "--limit-gib",
        type=float,
        default=float(os.environ.get("SWARM_TARGET_LIMIT_GIB") or DEFAULT_LIMIT_GIB),
        help=f"size bound in GiB (default: $SWARM_TARGET_LIMIT_GIB, else {DEFAULT_LIMIT_GIB:g})",
    )
    parser.add_argument("--dry-run", action="store_true", help="report what would be evicted")
    args = parser.parse_args(argv)
    if args.limit_gib <= 0:
        parser.error("--limit-gib must be greater than zero")
    prune(args.target, int(args.limit_gib * GIB), time.time(), args.dry_run)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
