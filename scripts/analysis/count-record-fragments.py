#!/usr/bin/env python3
"""Counts tasks that name another record by PART of its id.

WHY THIS EXISTS AS A SCRIPT RATHER THAN A NUMBER IN A TICKET. Task
01a0a585-9595-7b30-80b5-b4b0c700edef asks for evidence that a write-time notice
changes what workers write. That is a behaviour, and a behaviour needs a before
and an after measured the same way. A number quoted once in prose cannot be
re-taken; this can.

⚠️ TOKENISE EXACTLY AS THE SHIPPED SCANNER DOES -- split on anything that is not
hex or a dash -- and not with a loose regex. The first version of this used
`[0-9a-f][0-9a-f-]{7,34}`, which matched a 35-character PREFIX of a correctly
written 36-character id and counted it as a truncation. It reported 817 tasks
where the honest answer is 701, and it would have turned people doing it right
into evidence of a problem.

BASELINE, taken 2026-09-15 before the notice existed, over 1,207 tasks:
    701 carry a fragment that resolves            (58%)
    138 carry a fragment matching MORE than one record
The worst single case cites `01a015f8-3e6b` -- thirteen characters -- which
matches SEVEN records, because UUIDv7 spends its first twelve hex digits on a
millisecond timestamp and those seven were filed in the same millisecond.

Usage: count-record-fragments.py [path/to/swarm.sqlite3]
Opened read-only. The live database has a writer; do not point anything that
writes at it.
"""

import collections
import datetime
import re
import sqlite3
import sys

DEFAULT_DB = "/home/bschleifer/.local/state/swarm/swarm.sqlite3"
SPLIT = re.compile(r"[^0-9a-f-]+")


def fragments(text, task_ids, decision_ids):
    """Resolvable id fragments in one piece of text, with their match counts."""
    found = set()
    for raw in SPLIT.split((text or "").lower()):
        token = raw.strip("-")
        # Under eight characters is too short to resolve to anything a reader
        # should act on; 36 is a whole id and needs no help. A fragment starting
        # with a letter is skipped for the same reason the scanner skips it:
        # every id here is UUIDv7 and begins with a digit, and prose made of hex
        # letters would otherwise cost a lookup each.
        if len(token) < 8 or len(token) >= 36 or not token[0].isdigit():
            continue
        matches = [i for i in task_ids if i.startswith(token)]
        matches += [i for i in decision_ids if i.startswith(token)]
        if matches:
            found.add((token, len(matches)))
    return found


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_DB
    db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    task_ids = sorted(row[0] for row in db.execute("SELECT id FROM tasks"))
    decision_ids = sorted(row[0] for row in db.execute("SELECT id FROM decision_requests"))

    per_week = collections.Counter()
    filed_per_week = collections.Counter()
    ambiguous = []
    carrying = 0
    rows = list(db.execute("SELECT id, created_at, title, description FROM tasks"))
    for task_id, created_at, title, description in rows:
        week = datetime.datetime.fromtimestamp(created_at, datetime.UTC).strftime("%G-W%V")
        filed_per_week[week] += 1
        found = fragments(f"{title} {description}", task_ids, decision_ids)
        if not found:
            continue
        carrying += 1
        per_week[week] += 1
        if any(count > 1 for _, count in found):
            ambiguous.append((task_id, sorted(found)))

    print(f"tasks scanned: {len(rows)}")
    print(f"carrying a resolvable fragment: {carrying} ({carrying * 100 // max(1, len(rows))}%)")
    print(f"carrying an AMBIGUOUS fragment: {len(ambiguous)}")
    print("\nby week (with a fragment / filed):")
    for week in sorted(filed_per_week):
        print(f"  {week}: {per_week[week]:4d} / {filed_per_week[week]:4d}")
    print("\nworst ambiguity:")
    worst = sorted(
        ((token, count, task) for task, found in ambiguous for token, count in found if count > 1),
        key=lambda entry: entry[1],
        reverse=True,
    )
    for token, count, task in worst[:5]:
        print(f"  {token} matches {count} records, written in {task}")


if __name__ == "__main__":
    main()
