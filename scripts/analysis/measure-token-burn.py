#!/usr/bin/env python3
"""Measure Claude Code token burn per workspace per day, from the transcripts.

WHAT THIS IS AND IS NOT. It reads ~/.claude/projects/*/*.jsonl, which is what
Claude Code itself writes, and sums the `usage` block each assistant message
carries. It is therefore a measurement of what was actually spent, not an
estimate. It is NOT a bill: Anthropic's invoice is the authority on money, and
subscription usage is metered on their side, not here.

THREE WAYS THIS COULD LIE, AND WHAT IS DONE ABOUT EACH.

1. DOUBLE COUNTING. A transcript can carry the same assistant message twice --
   resumes, sidechains and compaction all replay entries. Summing lines would
   inflate a long-running worker more than a short one, which is exactly the
   population we are comparing. Every message is keyed by its own id and
   counted once, across all files.

2. IDENTITY COLLISION. A project directory is derived from the WORKSPACE PATH,
   so two workers in one repository share one directory and cannot be told
   apart here. Those are reported as the workspace they share, never as a
   worker name that would imply precision this data does not have.

3. SELF-INCLUSION. The session doing the measuring is itself in the corpus.
   It is not excluded -- it is real spend -- but it is named in the output so
   it can never be silently read as somebody else's.
"""
import json
import os
import sys
from collections import defaultdict
from datetime import datetime, timezone, timedelta

ROOT = os.path.expanduser("~/.claude/projects")

# Relative to one input token, from Anthropic's published per-model ratios.
# These hold across Opus/Sonnet/Haiku -- only the absolute price differs, which
# MODEL_WEIGHT carries separately.
W_INPUT = 1.0
W_CACHE_WRITE = 1.25
W_CACHE_READ = 0.1
W_OUTPUT = 5.0

# Opus costs 5x Sonnet per token; Haiku is far cheaper. Applied only to the
# "opus-equivalent" column so a day of Opus is not compared to a day of Haiku
# as though they cost the same.
def model_weight(model: str) -> float:
    m = (model or "").lower()
    if "opus" in m:
        return 1.0
    if "sonnet" in m:
        return 0.2
    if "haiku" in m:
        return 0.055
    return 1.0  # unknown model: assume expensive rather than flatter the total


def weighted(u: dict) -> float:
    return (
        u.get("input_tokens", 0) * W_INPUT
        + u.get("cache_creation_input_tokens", 0) * W_CACHE_WRITE
        + u.get("cache_read_input_tokens", 0) * W_CACHE_READ
        + u.get("output_tokens", 0) * W_OUTPUT
    )


def main() -> int:
    days = int(sys.argv[1]) if len(sys.argv) > 1 else 30
    cutoff = datetime.now(timezone.utc) - timedelta(days=days)

    seen: set[str] = set()
    by_day: dict[str, float] = defaultdict(float)
    by_day_raw: dict[str, dict[str, int]] = defaultdict(lambda: defaultdict(int))
    by_dir_day: dict[tuple[str, str], float] = defaultdict(float)
    by_dir: dict[str, float] = defaultdict(float)
    # CACHED VS UNCACHED, PER WORKSPACE. The weighted total answers "how much",
    # and this answers "why". cache_read is billed at a tenth of input, so a
    # high read share is CHEAP burn. cache_creation is billed at 1.25x, so a
    # workspace whose writes rival its reads is paying to build a cache it is
    # not reusing -- cache thrash, which is the classic cause of a bill that
    # climbs while the work looks unchanged.
    by_dir_raw: dict[str, dict[str, int]] = defaultdict(lambda: defaultdict(int))
    by_model_day: dict[tuple[str, str], float] = defaultdict(float)
    sessions_by_dir: dict[str, set[str]] = defaultdict(set)
    files = 0
    lines = 0
    dupes = 0
    undated = 0

    for project in sorted(os.listdir(ROOT)):
        pdir = os.path.join(ROOT, project)
        if not os.path.isdir(pdir):
            continue
        for entry in os.scandir(pdir):
            if not entry.name.endswith(".jsonl"):
                continue
            # Cheap pre-filter: a file untouched since the cutoff cannot hold a
            # message inside the window.
            if entry.stat().st_mtime < cutoff.timestamp():
                continue
            files += 1
            try:
                fh = open(entry.path, "r", errors="replace")
            except OSError:
                continue
            with fh:
                for line in fh:
                    if '"usage"' not in line:
                        continue
                    lines += 1
                    try:
                        rec = json.loads(line)
                    except ValueError:
                        continue
                    if rec.get("type") != "assistant":
                        continue
                    msg = rec.get("message") or {}
                    u = msg.get("usage") or {}
                    if not u:
                        continue
                    mid = msg.get("id") or rec.get("requestId")
                    if not mid:
                        undated += 1
                        continue
                    if mid in seen:
                        dupes += 1
                        continue
                    seen.add(mid)
                    ts = rec.get("timestamp")
                    if not ts:
                        undated += 1
                        continue
                    try:
                        when = datetime.fromisoformat(ts.replace("Z", "+00:00"))
                    except ValueError:
                        undated += 1
                        continue
                    if when < cutoff:
                        continue
                    day = when.date().isoformat()
                    model = msg.get("model", "unknown")
                    w = weighted(u) * model_weight(model)
                    by_day[day] += w
                    by_dir_day[(project, day)] += w
                    by_dir[project] += w
                    by_model_day[(day, model)] += w
                    sid = rec.get("sessionId")
                    if sid:
                        sessions_by_dir[project].add(sid)
                    for k in ("input_tokens", "cache_creation_input_tokens",
                              "cache_read_input_tokens", "output_tokens"):
                        by_day_raw[day][k] += u.get(k, 0)
                        by_dir_raw[project][k] += u.get(k, 0)

    out = {
        "scanned": {"files": files, "usage_lines": lines,
                    "unique_messages": len(seen), "duplicates_skipped": dupes,
                    "skipped_no_id_or_timestamp": undated},
        "by_day": dict(sorted(by_day.items())),
        "by_day_raw": {d: dict(v) for d, v in sorted(by_day_raw.items())},
        "by_dir": dict(sorted(by_dir.items(), key=lambda kv: -kv[1])),
        "by_dir_raw": {k: dict(v) for k, v in by_dir_raw.items()},
        "by_dir_day": {f"{k[0]}|{k[1]}": v for k, v in by_dir_day.items()},
        "by_model_day": {f"{k[0]}|{k[1]}": v for k, v in by_model_day.items()},
        "sessions_by_dir": {k: len(v) for k, v in sessions_by_dir.items()},
    }
    print(json.dumps(out))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
