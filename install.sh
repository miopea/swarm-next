#!/bin/sh
# Installs Swarm, or upgrades an existing install, from the release its own
# manifest currently offers.
#
# The manifest is the same document every running Hive reads to decide whether
# an update exists, so a fresh install and an upgrade can never disagree about
# what "current" means. Withdrawing a release means removing it from the
# manifest, and that stops new installs taking it as well as old ones.
#
# What this can and cannot prove: install.sh and the manifest come from the same
# repository, so this script cannot verify the manifest's signature in any way
# that means something — a key fetched beside the document it checks proves
# nothing. It verifies the artifact's digest against the manifest, which is
# worth doing because the artifact comes from a different host. Everything after
# this point is properly verified: the installed binary carries the release
# verifying key compiled in, and checks every future update against it.
set -eu

MANIFEST_URL="${SWARM_RELEASE_MANIFEST_URL:-https://raw.githubusercontent.com/miopea/swarm-next/main/releases.json}"

fail() {
	echo "install: $1" >&2
	exit 1
}

need() {
	command -v "$1" >/dev/null 2>&1 || fail "$1 is required and was not found"
}

need curl
need tar
need uname

case "$(uname -s)/$(uname -m)" in
	Linux/x86_64) platform="linux-x86_64" ;;
	*) fail "Swarm publishes linux-x86_64 today; this machine is $(uname -s)/$(uname -m)" ;;
esac

[ "$(id -u)" -eq 0 ] && fail "run this as the user who will own the Hive, not as root"

if command -v sha256sum >/dev/null 2>&1; then
	digest_of() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
	digest_of() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
	fail "sha256sum or shasum is required to verify the download"
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT INT TERM

echo "Reading $MANIFEST_URL"
curl -fsSL "$MANIFEST_URL" -o "$work/releases.json" \
	|| fail "the release manifest could not be read"

# Pulls (version, url, digest) out of each release the manifest offers and takes
# the newest by version order — not by position in the list, and not by string
# order, which would put 0.9.0 above 0.10.0.
#
# Every JSON token goes on its own line first so this does not depend on how the
# manifest happens to be laid out. Matching whole lines against a pretty-printed
# document worked until the same document arrived minified, and then silently
# returned the wrong release rather than failing.
offer="$(tr '{},' '\n\n\n' < "$work/releases.json" | awk '
	function value(line) {
		sub(/^[^:]*:[[:space:]]*"/, "", line)
		sub(/".*$/, "", line)
		return line
	}
	/"version"[[:space:]]*:/         { v = value($0) }
	/"artifact_url"[[:space:]]*:/    { u = value($0) }
	/"artifact_sha256"[[:space:]]*:/ { s = value($0) }
	v != "" && u != "" && s != "" {
		print v " " u " " s
		v = ""; u = ""; s = ""
	}
' | sort -V | tail -1)"

[ -n "$offer" ] || fail "the release manifest offers nothing this script understands"

version="$(echo "$offer" | cut -d' ' -f1)"
url="$(echo "$offer" | cut -d' ' -f2)"
expected="$(echo "$offer" | cut -d' ' -f3)"

case "$url" in
	https://*) ;;
	*) fail "the manifest names a non-https artifact; refusing" ;;
esac
case "$url" in
	*"$platform"*) ;;
	*) fail "the current release has no $platform build" ;;
esac

echo "Downloading Swarm $version"
curl -fsSL "$url" -o "$work/swarm.tar.gz" || fail "the release could not be downloaded"

actual="$(digest_of "$work/swarm.tar.gz")"
[ "$actual" = "$expected" ] || fail "the download does not match the digest the manifest published; refusing to install"

tar -xzf "$work/swarm.tar.gz" -C "$work" || fail "the release could not be unpacked"
bundle="$(find "$work" -maxdepth 1 -type d -name 'swarm-*' | head -1)"
[ -n "$bundle" ] && [ -x "$bundle/swarm-package" ] || fail "the release does not contain swarm-package"

# A first install asks which token to sign in with, and `curl … | sh` leaves
# this script's stdin attached to the pipe rather than the operator. Hand the
# terminal back where there is one; where there is not, swarm-package generates
# a token and prints it.
# Tested by opening it in a subshell.
#
# `[ -r /dev/tty ]` is true in contexts where the open then fails with "No such
# device or address". Redirecting in the condition itself is worse: dash treats
# a failed redirection as a fatal shell error rather than a false condition, so
# the script exited 2 with no message, after the download had already
# succeeded. A subshell turns the same failure into an ordinary exit status.
if (exec < /dev/tty) 2>/dev/null; then
	sh "$bundle/swarm-package" install "$bundle" < /dev/tty
else
	sh "$bundle/swarm-package" install "$bundle"
fi
