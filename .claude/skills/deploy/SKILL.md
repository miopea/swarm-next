---
name: deploy
description: Cuts and publishes a swarm-next release end to end — verify, bump, tag, build, GitHub release, sign and publish the manifest, confirm propagation. Use when the operator says "deploy", "ship it", "cut a release", or asks for a new version or tarball for the devs. Not for "commit" (commit and push only) or "reload" (rebuild this Hive locally, nothing published).
---

# Deploy — the full release, in order, with the reasons

The operator's vocabulary, settled 2026-08-26:

| Word | Means |
| --- | --- |
| **commit** | commit and push to main. Nothing else. |
| **reload** | rebuild this Hive from the checkout and restart. Local, nothing published. |
| **deploy** | everything below. A release that reaches people. |

Most changes are commit-and-reload. A version is cut occasionally, not at the
end of every change — so do not reach for this unless the word was used.

## The order matters, and here is why

`build-release.sh` **refuses an untagged commit**, and refuses a tag whose
version disagrees with `Cargo.toml`. That is a deliberate gate. It forces
verify → bump → tag → build, and you cannot shortcut it.

## 1. Verify — with CI's exact line

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all --check
CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets --all-features --color never -- -D warnings
echo "clippy exit=$?"     # the ONLY thing that decides
CARGO_INCREMENTAL=0 cargo test --workspace --color never
```

**Read the exit code. Never grep the output.** On 2026-08-26 the check was
`grep -E "^error"` against cargo's *colourised* output — every line begins with
an ANSI escape, so the pattern matched nothing and reported success for hours.
Ten real findings shipped in 0.8.13, 0.8.14 and 0.8.15 behind it.

**Use CI's flags, not an approximation.** Local runs used `--all-targets` and
CI uses `--all-targets --all-features`. Different flags measure different code.

**Re-run after the last edit.** A clean run followed by one more commit is not a
clean release; that put a `too_many_lines` error on main.

`CARGO_INCREMENTAL=0` is required in this workspace — six rustc ICEs came from
the incremental cache, each masking real errors.

### And check CI — on the commit you are releasing, not on whatever main is

Local green and CI red means the two are measuring different things, and that
gap is exactly what shipped ten findings across three releases.

**CI CANNOT HAVE RUN ON WORK YOU HAVE NOT PUSHED.** 0.8.18 was cut with 33
commits unpushed: `origin/main` was 32 behind, its four checks were green, and
every one of them described work from hours earlier. Reading that as "CI is
green" is the same false green this file already warns about, arriving through
a different door — the answer was real, it was just to a different question.

So the order is **push, wait, check, THEN tag**. Push first (step 3 up to the
push, without the tag), then:

```bash
# The commit CI must have run on is the one you are about to tag.
HEAD_SHA=$(git rev-parse HEAD)
gh api repos/miopea/swarm-next/commits/main --jq '.sha'   # must equal HEAD_SHA
until [ "$(gh api repos/miopea/swarm-next/commits/main/check-runs \
           --jq '[.check_runs[] | select(.status != "completed")] | length')" = "0" ]; do
  sleep 20
done
gh api repos/miopea/swarm-next/commits/main/check-runs \
  --jq '.check_runs[] | "\(.name): \(.conclusion)"'
```

**Compare the sha before you believe the conclusions.** A green result from an
ancestor is not a green result for your release, and nothing in the output says
which commit it belongs to.

**Do not tag while `rust` is failing.** A pending run is not a failure — wait
for it, as the loop above does, or note plainly that you released without
waiting and why. A red one means your local check missed something; find out
what before adding a tag, because a tag is awkward to walk back.

## 2. Bump — then READ IT BACK

Edit `version` in the root `Cargo.toml` (eight crates share it), then let cargo
refresh the lock:

```bash
NEW=0.8.19
# Address the FIELD, not a line number. Anchored to the start of a line so it
# cannot touch a version inside a dependency table.
sed -i -E "0,/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"$/s//version = \"$NEW\"/" Cargo.toml

# THE BUMP IS NOT DONE UNTIL IT IS READ BACK.
GOT=$(sed -n -E 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)"$/\1/p' Cargo.toml | head -1)
[ "$GOT" = "$NEW" ] || { echo "bump did not apply: Cargo.toml says $GOT" >&2; exit 1; }

CARGO_INCREMENTAL=0 cargo check --workspace --quiet
```

**A LINE-ADDRESSED `sed` MATCHES NOTHING AND SAYS NOTHING.** The previous
version of this step was `sed -i '15s/...'`, and the version line has moved
since it was written. During 0.8.18 it was run against line 14, changed
nothing, and reported success — caught only because `git diff --stat` came back
empty. A release then goes out carrying the PREVIOUS version number while every
log line says the bump worked.

That is the failure shape this repository keeps producing: a check that cannot
fail. Same as the reconcile that compares a symlink against itself, and the
schema tests that passed against empty databases. The guard is reading the
value back, not writing it more carefully.

Ask the operator for the version if it is anything other than a patch bump.
Patch is right almost always, and wrong exactly when it matters.

### If this release carries a schema migration, say the number out loud

```bash
git diff --unified=0 "v$(git tag --list 'v*' --sort=-v:refname | head -1 | tr -d v)"..HEAD \
  -- crates/swarm-persistence/src/lib.rs | grep -E '^\+const .*_SCHEMA_VERSION' || echo "none"
```

0.8.18 went out as a PATCH carrying migrations 96 through 102, one of which
rebuilds `worker_profiles` — the table seventeen others hold foreign keys into,
and one that had already failed once on the operator's real database. Nothing
in this procedure noticed, because a patch bump asks no questions.

If that command prints anything, tell the operator the range and the version
you intend, and get an explicit answer before tagging. This is not a versioning
policy — it is refusing to make the choice silently.

## 3. Commit, tag, push

```bash
git add Cargo.toml Cargo.lock
git commit -m "release: X.Y.Z"      # say what a Hive taking it gets
git tag -a vX.Y.Z -m "Swarm X.Y.Z"
git push origin main && git push origin vX.Y.Z
```

**Say in the message whether it carries a schema migration**, because the backup
guard covers a reload from a checkout and **not** a tarball install — an
upgrading Hive migrates unprotected. Tell the operator to copy their
`swarm.sqlite3` first when it does.

## 4. Build

```bash
eval "$(op-login)"
sh packaging/linux/build-release.sh      # prints the tarball path — read it, never guess
```

## 5. Verify the ARTIFACT, not your working tree

The tarball is what a Hive installs. A fix present in your checkout and absent
from the artifact ships nothing.

```bash
T=dist/swarm-X.Y.Z-linux-x86_64.tar.gz
tar -xzOf $T swarm-X.Y.Z-linux-x86_64/VERSION
tar -xzOf $T --wildcards 'swarm-X.Y.Z-linux-x86_64/web/assets/index-*.js' | grep -c 'X\.Y\.Z'
```

The second one matters: without the version baked into the bundle, the
stale-page notice ships **inert** and looks fine.

**Then check whatever this release exists to change.** 0.8.16 existed to ship two
systemd template lines; verifying them in the working tree would have proved
nothing. Extract the file from the tarball and look.

### If the release exists to add a guard, run the guard

0.8.16 shipped a packaging check and nothing confirmed the check itself ran. A
guard that has never fired is a guard nobody has tested.

```bash
sh -n packaging/linux/test-package-lifecycle.sh   # it parses
# and prove it BITES: break the thing it guards, confirm it fails, restore
```

An ablation is the only evidence a guard works. A guard that passes on correct
input has told you nothing.

## 6. GitHub release

```bash
gh release create vX.Y.Z $T --title "Swarm X.Y.Z" --notes "..."
```

Notes say what a Hive taking it gets, and name the migration if there is one.

## 7. Sign the manifest

The key lives in 1Password and exists on disk only for this command.
`/run/user/$UID` is tmpfs, so it never reaches a disk.

```bash
eval "$(op-login)"
umask 077
KEY=/run/user/$(id -u)/swarm-signing.key
op read --account my "op://BFG/Swarm release signing key/credential" > "$KEY"
sh packaging/linux/publish-release-manifest.sh "$KEY" \
  https://github.com/miopea/swarm-next/releases/download/vX.Y.Z \
  $T > releases.json
rm -f "$KEY"
```

The base URL **must include the `/vX.Y.Z/` segment**. Omitting it is how 0.8.7
published a manifest pointing at a 404 and stranded a developer on a download
error.

The manifest states what is offered **now** — it rewrites the whole document from
the tarballs named, so a release is withdrawn by ceasing to list it.

## 8. Fetch the artifact through the manifest's own URL

```bash
URL=$(python3 -c "import json;print(json.load(open('releases.json'))['payload']['releases'][0]['artifact_url'])")
curl -sSL -o /tmp/dl.tar.gz -w '%{http_code}\n' "$URL"
sha256sum /tmp/dl.tar.gz
python3 -c "import json;print(json.load(open('releases.json'))['payload']['releases'][0]['artifact_sha256'])"
```

Expect 200 and identical hashes. This is the step that catches a manifest
pointing somewhere wrong, and it is the reason 0.8.7 cannot happen again.

## 9. Publish

```bash
git add releases.json && git commit -m "release: offer X.Y.Z to Hives" && git push origin main
gh api repos/miopea/swarm-next/contents/releases.json --jq '.content' | base64 -d | head -8
```

Publishing the manifest is what makes a release real. A tarball on GitHub and
absent from the manifest is offered to nobody.

## 10. Propagation — published is not live

```bash
curl -sS -H 'Cache-Control: no-cache' \
  https://raw.githubusercontent.com/miopea/swarm-next/main/releases.json | head -6
```

**This tells you about YOUR edge and nobody else's.** On 2026-08-26 this machine
saw the new version immediately while a developer's machine served the previous
one for about twenty minutes, and she was told to update something that was not
yet being offered to her.

Report it as **"published; propagation may lag"**, never "live and installable",
unless you have checked from the machine that matters.

## Never guess a path

Twice on 2026-08-26 a download directory was guessed instead of listed, and both
guesses were wrong in front of the operator. `build-release.sh` prints the
tarball path. `ls` the downloads directory. Print what you found.

## Afterwards

Record deployment evidence against any task this release closes, and say
plainly what was **not** proven — a released fix that nobody has watched work is
released, not verified.
