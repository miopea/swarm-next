#!/bin/sh
# Real installed CLI/SQLite preflight drill. No restore or service commands.
set -eu
umask 077
cli=${SWARM_TEST_SWARMCTL:-"$HOME/.local/lib/swarm/current/bin/swarmctl"}
[ -x "$cli" ] || { echo 'Installed swarmctl is unavailable' >&2; exit 1; }
drill=$(mktemp -d /tmp/swarm-backup-preflight.XXXXXXXX)
cleanup() {
  case "$drill" in /tmp/swarm-backup-preflight.*) rm -f -- "$drill"/*; rmdir -- "$drill";; esac
}
trap cleanup EXIT HUP INT TERM

# A verifier must never initialize its own missing/empty evidence.
if "$cli" verify-database "$drill/missing.sqlite3" >"$drill/check.log" 2>&1; then
  echo 'FAIL: missing database passed verification' >&2; exit 1
fi
[ ! -e "$drill/missing.sqlite3" ]
: > "$drill/empty.sqlite3"
if "$cli" verify-database "$drill/empty.sqlite3" >"$drill/check.log" 2>&1; then
  echo 'FAIL: empty database passed verification' >&2; exit 1
fi
[ ! -s "$drill/empty.sqlite3" ]

token=$(sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$HOME/.config/swarm/swarm.env")
[ -n "$token" ] || { echo 'Operator credential unavailable' >&2; exit 1; }
printf 'header = "Authorization: Bearer %s"\n' "$token" > "$drill/auth"
unset token
curl --config "$drill/auth" --fail --silent --show-error --max-time 120 \
  --max-filesize 2147483648 --output "$drill/good.sqlite3" \
  http://127.0.0.1:8766/api/v1/backups/database
rm -f -- "$drill/auth"
timeout 60 "$cli" verify-database "$drill/good.sqlite3" >"$drill/check.log" 2>&1
[ "$(sqlite3 -readonly "$drill/good.sqlite3" 'PRAGMA integrity_check;')" = ok ]
bytes=$(stat -c %s "$drill/good.sqlite3")
[ "$bytes" -gt 8192 ]
cp "$drill/good.sqlite3" "$drill/truncated.sqlite3"
truncate -s 8192 "$drill/truncated.sqlite3"
before=$(sha256sum "$drill/truncated.sqlite3" | cut -d ' ' -f 1)
rejected=0
timeout 60 "$cli" verify-database "$drill/truncated.sqlite3" >"$drill/check.log" 2>&1 || rejected=$?
if [ "$rejected" -eq 0 ]; then
  echo 'FAIL: truncated database passed verification' >&2; exit 1
fi
[ "$rejected" -eq 1 ] || { echo 'FAIL: verifier did not return a normal rejection' >&2; exit 1; }
after=$(sha256sum "$drill/truncated.sqlite3" | cut -d ' ' -f 1)
[ "$before" = "$after" ]
printf '{"result":"passed","scope":"installed_cli_backup_preflight","export_bytes":%s,"missing_rejected":true,"empty_rejected":true,"valid_export_verified":true,"truncation_rejected_without_mutation":true,"restore_performed":false}\n' "$bytes"
exit 0
