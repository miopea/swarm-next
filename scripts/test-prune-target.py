#!/usr/bin/env python3
"""Tests for prune-target.py, against target directories built here.

Run: python3 scripts/test-prune-target.py
"""

from __future__ import annotations

import fcntl
import importlib.util
import io
import os
import sys
import tempfile
import time
import unittest
from contextlib import redirect_stdout

# Loading the script as a module would otherwise leave scripts/__pycache__.
sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location("prune_target", os.path.join(HERE, "prune-target.py"))
assert SPEC is not None and SPEC.loader is not None
prune_target = importlib.util.module_from_spec(SPEC)
# A dataclass looks its module up by name while it is being defined.
sys.modules[SPEC.name] = prune_target
SPEC.loader.exec_module(prune_target)

# The real clock, because the filesystem stamps an access with it.
NOW = time.time()
DAY = 24 * 60 * 60
KIB = 1024
UNIT_BYTES = 64 * KIB


def write(path: str, size: int = 0) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as handle:
        handle.write(b"x" * size)


def age(paths: list[str], seconds: float) -> None:
    """Dates every path, and everything under it, `seconds` before NOW.

    Everything is listed before anything is dated, because listing a directory
    sets its access time again.
    """
    entries = []
    for path in paths:
        entries.append(path)
        for root, dirs, files in os.walk(path):
            entries.extend(os.path.join(root, name) for name in dirs + files)
    for entry in entries:
        os.utime(entry, (NOW - seconds, NOW - seconds), follow_symlinks=False)


def unit(profile: str, name: str, digest: str, used_days_ago: float) -> list[str]:
    """Lays out one build unit the way cargo does, last used that long ago."""
    paths = [
        os.path.join(profile, ".fingerprint", f"{name}-{digest}"),
        os.path.join(profile, "deps", f"lib{name}-{digest}.rlib"),
        os.path.join(profile, "deps", f"{name}-{digest}.d"),
        os.path.join(profile, "build", f"{name}-{digest}"),
    ]
    write(os.path.join(paths[0], f"lib-{name}"), 16)
    write(os.path.join(paths[0], f"lib-{name}.json"), 64)
    write(paths[1], UNIT_BYTES)
    write(paths[2], 64)
    write(os.path.join(paths[3], "out", "generated.rs"), 64)
    age(paths, used_days_ago * DAY)
    return paths


def exists(paths: list[str]) -> list[bool]:
    return [os.path.lexists(path) for path in paths]


class PruneTarget(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.target = os.path.join(self.scratch.name, "target")
        self.debug = os.path.join(self.target, "debug")
        os.makedirs(os.path.join(self.debug, ".fingerprint"))

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def prune(self, limit: int, dry_run: bool = False) -> tuple[int, str]:
        out = io.StringIO()
        with redirect_stdout(out):
            freed = prune_target.prune(self.target, limit, NOW, dry_run)
        return freed, out.getvalue()

    def test_nothing_is_removed_under_the_bound(self) -> None:
        old = unit(self.debug, "serde", "0123456789abcdef", used_days_ago=30)

        freed, out = self.prune(limit=10 * UNIT_BYTES)

        self.assertEqual(freed, 0)
        self.assertEqual(exists(old), [True] * 4)
        self.assertIn("under the", out)

    def test_the_least_recently_used_units_go_whole_until_three_quarters(self) -> None:
        oldest = unit(self.debug, "a", "000000000000000a", used_days_ago=10)
        older = unit(self.debug, "b", "000000000000000b", used_days_ago=9)
        old = unit(self.debug, "c", "000000000000000c", used_days_ago=8)
        newer = unit(self.debug, "d", "000000000000000d", used_days_ago=7)
        total = prune_target.disk_usage(self.target)
        sizes = {each.freed for each in prune_target.profile_units(self.debug)}
        self.assertEqual(len(sizes), 1)
        size = sizes.pop()
        # Three quarters of this bound falls between two and one units below
        # the total, so exactly the two oldest have to go.
        goal = total - size - size // 2
        limit = int(goal / 0.75) + 1
        self.assertGreater(total, limit)

        self.prune(limit=limit)

        self.assertEqual(exists(oldest) + exists(older), [False] * 8)
        self.assertEqual(exists(old) + exists(newer), [True] * 8)
        self.assertLessEqual(prune_target.disk_usage(self.target), int(limit * 0.75))

    def test_units_used_in_the_last_two_days_are_kept_even_over_the_bound(self) -> None:
        today = unit(self.debug, "a", "000000000000000a", used_days_ago=0.1)
        yesterday = unit(self.debug, "b", "000000000000000b", used_days_ago=1.9)

        freed, out = self.prune(limit=UNIT_BYTES)

        self.assertEqual(freed, 0)
        self.assertEqual(exists(today) + exists(yesterday), [True] * 8)
        self.assertIn("still over the bound", out)

    def test_a_recent_access_keeps_a_unit_built_long_ago(self) -> None:
        """Cargo reads a fingerprint it does not rebuild, so a dependency built
        weeks ago and read today is in use."""
        dependency = unit(self.debug, "serde", "0123456789abcdef", used_days_ago=40)
        fingerprint = os.path.join(dependency[0], "lib-serde")
        os.utime(fingerprint, (NOW - DAY / 2, NOW - 40 * DAY))

        self.prune(limit=UNIT_BYTES)

        self.assertEqual(exists(dependency), [True] * 4)

    def test_listing_a_unit_does_not_make_it_look_used(self) -> None:
        """Pruning lists every unit's directories, and listing a directory sets
        its access time. Were that read as use, no directory would ever go."""
        stale = unit(self.debug, "a", "000000000000000a", used_days_ago=10)
        os.listdir(stale[0])
        os.listdir(stale[3])

        self.prune(limit=UNIT_BYTES)

        self.assertEqual(exists(stale), [False] * 4)

    def test_a_running_build_keeps_its_profile_and_another_is_pruned(self) -> None:
        release = os.path.join(self.target, "release")
        os.makedirs(os.path.join(release, ".fingerprint"))
        building = unit(self.debug, "a", "000000000000000a", used_days_ago=10)
        idle = unit(release, "b", "000000000000000b", used_days_ago=10)
        # What cargo holds for the whole of a build.
        lock = os.open(os.path.join(self.debug, ".cargo-build-lock"), os.O_RDWR | os.O_CREAT)
        fcntl.flock(lock, fcntl.LOCK_EX)
        try:
            _, out = self.prune(limit=UNIT_BYTES)
        finally:
            fcntl.flock(lock, fcntl.LOCK_UN)
            os.close(lock)

        self.assertEqual(exists(building), [True] * 4)
        self.assertEqual(exists(idle), [False] * 4)
        self.assertIn(f"a build is running in {self.debug}", out)

    def test_the_locks_are_released_afterwards(self) -> None:
        unit(self.debug, "a", "000000000000000a", used_days_ago=10)

        self.prune(limit=UNIT_BYTES)

        for name in prune_target.LOCK_NAMES:
            fd = os.open(os.path.join(self.debug, name), os.O_RDWR)
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            finally:
                os.close(fd)

    def test_the_fingerprint_is_removed_first(self) -> None:
        """Interrupted partway, cargo must find no record of the unit rather
        than a record whose outputs are half gone."""
        paths = unit(self.debug, "a", "000000000000000a", used_days_ago=10)
        stale = prune_target.Unit(paths=sorted(paths, reverse=True))
        removed: list[str] = []
        original = prune_target.remove
        prune_target.remove = removed.append
        try:
            prune_target.evict(stale)
        finally:
            prune_target.remove = original

        self.assertEqual(removed[0], paths[0])
        self.assertEqual(sorted(removed), sorted(paths))

    def test_an_incremental_directory_is_a_unit_of_its_own(self) -> None:
        stale = os.path.join(self.debug, "incremental", "swarm_api-0nlww68p5n0hb")
        fresh = os.path.join(self.debug, "incremental", "swarm_api-1abcdefghijkl")
        write(os.path.join(stale, "s-session", "query-cache.bin"), UNIT_BYTES)
        write(os.path.join(fresh, "s-session", "query-cache.bin"), UNIT_BYTES)
        age([stale], 10 * DAY)
        age([fresh], DAY / 2)

        self.prune(limit=UNIT_BYTES)

        self.assertFalse(os.path.lexists(stale))
        self.assertTrue(os.path.lexists(fresh))

    def test_what_cargo_did_not_name_with_a_unit_hash_is_left_alone(self) -> None:
        unit(self.debug, "a", "000000000000000a", used_days_ago=10)
        kept = [
            os.path.join(self.target, "CACHEDIR.TAG"),
            os.path.join(self.target, "verify", "test.log"),
            os.path.join(self.debug, "swarm-api"),
            os.path.join(self.debug, "libswarm_api.rlib"),
            os.path.join(self.debug, "deps", "libunhashed.so"),
            os.path.join(self.debug, "deps", "name-0123456789abcdef-suffix"),
        ]
        for path in kept:
            write(path, UNIT_BYTES)
        age(kept, 30 * DAY)

        self.prune(limit=UNIT_BYTES)

        self.assertEqual(exists(kept), [True] * len(kept))

    def test_a_hard_linked_binary_is_not_counted_as_freed(self) -> None:
        """Cargo copies a finished binary up beside deps/ as a hard link, so
        removing the deps/ copy frees nothing while the other link remains."""
        paths = unit(self.debug, "a", "000000000000000a", used_days_ago=10)
        binary = os.path.join(self.debug, "deps", "swarm_api-000000000000000a")
        write(binary, 4 * UNIT_BYTES)
        os.link(binary, os.path.join(self.debug, "swarm-api"))
        age(paths + [binary], 10 * DAY)

        freed, _ = self.prune(limit=UNIT_BYTES)

        self.assertFalse(os.path.lexists(binary))
        self.assertTrue(os.path.lexists(os.path.join(self.debug, "swarm-api")))
        self.assertLess(freed, 4 * UNIT_BYTES)
        self.assertGreaterEqual(freed, UNIT_BYTES)

    def test_a_dry_run_removes_nothing(self) -> None:
        stale = unit(self.debug, "a", "000000000000000a", used_days_ago=10)

        freed, out = self.prune(limit=UNIT_BYTES, dry_run=True)

        self.assertGreaterEqual(freed, UNIT_BYTES)
        self.assertEqual(exists(stale), [True] * 4)
        self.assertIn("would evict 1", out)
        self.prune(limit=100 * UNIT_BYTES, dry_run=True)
        self.assertFalse(os.path.exists(self.log()), "a dry run recorded a run that changed nothing")

    def log(self) -> str:
        return os.path.join(self.target, prune_target.LOG_NAME)

    def test_each_run_is_recorded_with_its_sizes(self) -> None:
        stale = unit(self.debug, "a", "000000000000000a", used_days_ago=10)
        write(stale[1], 4 * 1024 * KIB)
        age(stale, 10 * DAY)
        self.prune(limit=100 * 1024 * KIB)
        self.prune(limit=UNIT_BYTES)

        with open(self.log(), encoding="utf-8") as handle:
            under, over = handle.read().splitlines()
        self.assertIn("evicted=0 busy_profiles=0", under)
        self.assertIn("evicted=1 busy_profiles=0", over)
        before = int(over.split("before_mib=")[1].split()[0])
        after = int(over.split("after_mib=")[1].split()[0])
        self.assertEqual((before, after), (4, 0))

    def test_the_record_is_kept_to_the_newest_runs(self) -> None:
        original = prune_target.LOG_RUNS
        prune_target.LOG_RUNS = 3
        try:
            for _ in range(7):
                self.prune(limit=100 * UNIT_BYTES)
        finally:
            prune_target.LOG_RUNS = original

        with open(self.log(), encoding="utf-8") as handle:
            self.assertEqual(len(handle.read().splitlines()), 3)

    def test_a_record_that_cannot_be_written_does_not_stop_a_prune(self) -> None:
        stale = unit(self.debug, "a", "000000000000000a", used_days_ago=10)
        os.makedirs(self.log())

        _, out = self.prune(limit=UNIT_BYTES)

        self.assertEqual(exists(stale), [False] * 4)
        self.assertIn("could not record this run", out)

    def test_a_profile_inside_a_target_triple_is_found(self) -> None:
        nested = os.path.join(self.target, "x86_64-unknown-linux-gnu", "debug")
        os.makedirs(os.path.join(nested, ".fingerprint"))
        stale = unit(nested, "a", "000000000000000a", used_days_ago=10)

        self.prune(limit=UNIT_BYTES)

        self.assertEqual(exists(stale), [False] * 4)

    def test_a_profile_reached_through_a_link_is_left_alone(self) -> None:
        """A link inside target/ can lead anywhere; pruning stays inside."""
        elsewhere = os.path.join(self.scratch.name, "elsewhere", "debug")
        os.makedirs(os.path.join(elsewhere, ".fingerprint"))
        outside = unit(elsewhere, "a", "000000000000000a", used_days_ago=10)
        os.makedirs(os.path.join(self.target, "x86_64-unknown-linux-gnu"))
        os.symlink(elsewhere, os.path.join(self.target, "x86_64-unknown-linux-gnu", "debug"))
        os.symlink(os.path.dirname(elsewhere), os.path.join(self.target, "linked"))

        self.prune(limit=1)

        self.assertEqual(exists(outside), [True] * 4)

    def test_a_unit_directory_reached_through_a_link_is_left_alone(self) -> None:
        """A real profile's deps/ or incremental/ could itself be a link that
        leads out of target/; what it leads to is not this target's to prune."""
        elsewhere = os.path.join(self.scratch.name, "elsewhere")
        outside = [
            os.path.join(elsewhere, ".fingerprint", "far-000000000000000f"),
            os.path.join(elsewhere, "deps", "libfar-000000000000000f.rlib"),
            os.path.join(elsewhere, "build", "far-000000000000000f"),
            os.path.join(elsewhere, "examples", "far-000000000000000f"),
            os.path.join(elsewhere, "incremental", "far-0nlww68p5n0hb"),
        ]
        for path in outside:
            write(os.path.join(path, "data") if not path.endswith(".rlib") else path, UNIT_BYTES)
        age([elsewhere], 10 * DAY)
        release = os.path.join(self.target, "release")
        os.makedirs(release)
        for name in (".fingerprint", "deps", "build", "examples", "incremental"):
            os.symlink(os.path.join(elsewhere, name), os.path.join(release, name))
        inside = unit(self.debug, "a", "000000000000000a", used_days_ago=10)

        self.prune(limit=1)

        self.assertEqual(exists(outside), [True] * 5)
        self.assertEqual(exists(inside), [False] * 4)

    def test_a_missing_target_is_not_an_error(self) -> None:
        out = io.StringIO()
        with redirect_stdout(out):
            freed = prune_target.prune(os.path.join(self.scratch.name, "absent"), UNIT_BYTES, NOW)

        self.assertEqual(freed, 0)
        self.assertIn("does not exist", out.getvalue())


if __name__ == "__main__":
    unittest.main()
