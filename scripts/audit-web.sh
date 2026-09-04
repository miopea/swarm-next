#!/bin/sh
# pnpm audit, with the network failure told apart from the finding.
#
# ⚠️ THE PROBLEM THIS SOLVES IS A CHECK THAT CANNOT DISTINGUISH TWO ANSWERS.
# `pnpm audit` exits non-zero both when it finds a high-severity advisory and
# when it cannot reach registry.npmjs.org at all. Those are opposite results —
# one is "your dependencies are unsafe", the other is "I learned nothing" — and
# the required `web` check reported them identically.
#
# On 2026-09-04 the advisories endpoint timed out three times across two hours
# (`POST /-/npm/v1/security/advisories/bulk error (23)`, then `TimeoutError`),
# and a release sat blocked behind a check that had found nothing wrong with it.
# `enforce_admins` is false on this repo, so the real-world outcome of failing
# closed on an outage is an admin pushing past ALL FOUR required checks — which
# is strictly worse than this script passing one of them with a loud warning.
#
# So: a real advisory still fails, and always will. An unreachable database
# says so in as many words and does not pretend to be a verdict.
#
# ⚠️ AND THE BANNER IS BOUNDED, BECAUSE A PER-RUN NOTICE IS NOT A CONDITION.
# A check that exits 0 on an outage exits 0 FOREVER if the endpoint stays
# broken. Nobody reads a green check's log, so "this proved nothing" degrades
# into "this is green" by repetition — silent in exactly the way the
# fail-closed version was not. Queen named this before it shipped, comparing it
# to a wildcard-renewal automation on this fleet that exists, is purpose-built,
# and has never once executed: worse than an absence, because an absence would
# have been noticed.
#
# So a success stamps the date and an outage reads it. Tolerating a broken
# registry for a few days is a judgement; tolerating it forever is a defect.
#
# ⚠️ ONE LIMIT IS ACCEPTED RATHER THAN SOLVED, AND IT IS NOT A TODO.
# The stamp lives in the GitHub Actions cache, which is best-effort — evictable
# on the repo size limit or after disuse. If it is evicted DURING a sustained
# outage, this script finds no record and exits 0 saying so. Writing it every
# run keeps it warm and default-branch caches restore everywhere, so the hole
# is narrow; it is not closed. Reviewed and accepted with that stated.
#
# ⚠️ AND DO NOT "FIX" IT BY READING THE LAST SUCCESSFUL RUN FROM THE GITHUB API.
# That is the obvious improvement and it is UNSOUND, for precisely the reason
# this file exists: a run that exited 0 via the outage path below IS a
# successful run. The API would report the audit as fresh on the very days it
# did not happen — the banner problem with a nicer data source, measuring the
# thing that exited 0 rather than the thing that occurred. The cache stamp is
# not a weaker version of that answer; it is the only one of the two that
# records the right event.
#
# Committing the stamp from CI would be sound, and was declined on price: a
# workflow writing to its own repository buys loops, protected-branch friction
# and a bot commit on every audit day, in exchange for a best-effort marker.
set -eu

ATTEMPTS=3
# Where the last REACHED-THE-DATABASE run is recorded, and how long an outage
# may lean on it. The workflow restores and saves this across runs; locally it
# lands in the working tree and is gitignored.
STAMP=${AUDIT_STAMP:-.audit-web-last-success}
MAX_STALE_DAYS=${AUDIT_MAX_STALE_DAYS:-7}
# Backoff between retries. Configurable ONLY so the ablation can drive the
# outage path in seconds rather than minutes — a guard nobody can afford to
# test is a guard nobody tests.
RETRY_BASE=${AUDIT_RETRY_BASE_SECONDS:-15}
OUT=$(mktemp)
trap 'rm -f "$OUT"' EXIT

n=1
while [ "$n" -le "$ATTEMPTS" ]; do
  # --json so the ANSWER is a document we can read, not an exit code that means
  # two things. Exit status is captured and deliberately not acted on yet.
  set +e
  pnpm audit --prod --audit-level high --json > "$OUT" 2>/dev/null
  set -e

  # THE VERDICT IS "DID IT PARSE", NOT "DID IT EXIT 0". A timeout produces no
  # JSON at all, so parsing is what separates an answer from a non-answer.
  if node -e '
      const fs = require("fs");
      const raw = fs.readFileSync(process.argv[1], "utf8").trim();
      if (!raw) process.exit(1);
      const doc = JSON.parse(raw);
      const found = doc.metadata && doc.metadata.vulnerabilities
        ? (doc.metadata.vulnerabilities.high || 0) + (doc.metadata.vulnerabilities.critical || 0)
        : Object.keys(doc.advisories || {}).length;
      // Print the count for the log, then say so in the exit status.
      console.log(`pnpm audit reached the advisory database: ${found} high or critical`);
      process.exit(found > 0 ? 2 : 0);
    ' "$OUT"; then
    # Stamped ONLY here — on the path that actually read an advisory document.
    # Stamping anywhere else would make the freshness record report that the
    # check ran when what ran was the excuse.
    date +%s > "$STAMP"
    exit 0
  elif [ $? -eq 2 ]; then
    echo "--- advisories ---"
    cat "$OUT"
    echo "FAIL: pnpm audit found high or critical advisories in production dependencies." >&2
    exit 1
  fi

  echo "attempt $n of $ATTEMPTS: no advisory document came back; retrying in $((n * RETRY_BASE))s" >&2
  [ "$n" -lt "$ATTEMPTS" ] && sleep $((n * RETRY_BASE))
  n=$((n + 1))
done

# ⚠️ NOT A PASS. The audit did not run; nothing was checked, and this says so
# rather than printing a green tick over an unanswered question.
cat >&2 <<'WARN'
==============================================================================
ADVISORY DATABASE UNREACHABLE — THIS CHECK PROVED NOTHING.
registry.npmjs.org did not return an advisory document after 3 attempts, so no
dependency was examined. This is not evidence that the tree is clean.
==============================================================================
WARN

if [ ! -f "$STAMP" ]; then
  # No record either way. Leaning on a success we cannot show is the same
  # unearned confidence this script exists to refuse, so say which of the two
  # it is rather than implying the audit was fine recently.
  echo "No previous successful audit is on record, so how long this has been true is unknown." >&2
  exit 0
fi

LAST=$(cat "$STAMP")
AGE_DAYS=$(( ( $(date +%s) - LAST ) / 86400 ))
echo "Last audit that actually reached the database: $(date -d "@$LAST" -u '+%Y-%m-%d %H:%MZ') — ${AGE_DAYS}d ago." >&2

if [ "$AGE_DAYS" -ge "$MAX_STALE_DAYS" ]; then
  cat >&2 <<WARN
------------------------------------------------------------------------------
AND THAT IS TOO LONG. No advisory database has been reached in ${AGE_DAYS} days
(limit ${MAX_STALE_DAYS}). This stopped being an outage to wait out and became a
dependency audit that is not running. Failing so it is somebody's problem.
------------------------------------------------------------------------------
WARN
  exit 1
fi
exit 0
