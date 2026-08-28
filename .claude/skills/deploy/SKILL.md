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

## 2. Write the notes — this is the step that gets skipped

Add a section to `RELEASE_NOTES.md` for the version being cut:

```markdown
## X.Y.Z

### New features
- What a person can now do, in their words

### Fixes
- What stopped being broken, and what they will notice
```

`swarm-release notes` prefers this over the commit subjects, per release, and
falls back to generation for any version with no section. **Absent keeps
working** — a release that ships rough notes beats one that ships none.

**Write for the person running Swarm, not the person who wrote the code.** The
generated list is conventional-commit subjects, which is why 0.8.18 gave
operators 32 bullets, six of them schema migrations, and the operator asked for
"more human readable in future releases". Bullets may wrap; they are joined.

End a bullet with `(after the worker engine update)` when the change is
installed but not in effect until the engine swaps. The phrase is stripped and
becomes the flag, so it is not printed twice.

**The build prints which source each release used.** Read those lines:

```
swarm-release: 0.8.20 notes come from RELEASE_NOTES.md (4 entries)
swarm-release: 0.8.19 notes generated from commit subjects (32 entries); no RELEASE_NOTES.md section
```

Seeing "generated from commit subjects" for the version you are cutting means
your section heading does not match the version — notes written for 0.8.20 while
cutting 0.9.0 are silently ignored, and the output looks fine either way.

## 3. Check whether the terminal-host protocol moved

```bash
git diff v<previous>..HEAD -- crates/swarm-terminal/src/ipc.rs | grep PROTOCOL_VERSION
```

If `PROTOCOL_VERSION` changed, **say so in the release notes and the GitHub
release**, because a Hive installs it with a different command:

```bash
swarm-package migrate-protocol <bundle>     # NOT update
```

`update` refuses a protocol change on purpose — a host and an API speaking
different protocols is the failure that guard exists to prevent. The migration
drains the terminal host, defers if any worker session is live, and swaps the
API and the host together.

**Tell the operator their workers will be stopped.** A protocol migration is the
one install that cannot preserve running terminals, because both processes have
to move at once. Everything else in this file leaves workers alone.

On 2026-08-27 a protocol bump reached main without this step, every operator
reload died at the install guard for three hours, and the release was reverted
rather than migrated — the command existed the whole time and the error did not
name it.

## 4. Bump — then READ IT BACK

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

## 5. Commit, tag, push

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

## 6. Build

```bash
eval "$(op-login)"
sh packaging/linux/build-release.sh      # prints the tarball path — read it, never guess
```

## 7. Verify the ARTIFACT, not your working tree

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

## 8. GitHub release

```bash
gh release create vX.Y.Z $T --title "Swarm X.Y.Z" --notes "..."
```

Notes say what a Hive taking it gets, and name the migration if there is one.

## 9. Sign the manifest

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

### Offering to Hives whose protocol differs — `SWARM_OFFER_PROTOCOL`

**A Hive can only see offers its own installed code will show it**, and every
release up to 0.9.1 filters the manifest for offers whose protocol EQUALS its
compiled `PROTOCOL_VERSION`, reporting a miss as "current". So a release that
bumps the protocol is invisible to the field while telling operators they are up
to date. That is how v0.9.0 shipped, fully signed and verified, to nobody.

Discovery no longer filters on protocol from 0.9.2 onward — but that fix lives
in the installed binary, so it does nothing for a Hive already in the field.
**Until every Hive is past 0.9.1, list a release under every protocol you need
to reach:**

```bash
SWARM_OFFER_PROTOCOL=9,10 sh packaging/linux/publish-release-manifest.sh ...
```

One entry per protocol, all naming the same artifact and digest. Check both:

```bash
python3 -c "import json;d=json.load(open('releases.json'));print(sorted((r['version'],r['protocol']) for r in d['payload']['releases']))"
```

**Only sound when the field can actually install it.** The manifest's protocol
is a discovery filter and the install reads the bundle's own `PROTOCOL` file, so
this offers a release the Hive will then migrate onto — which is true only
because `update` performs the migration and `test-field-upgrade.sh` proves the
real `swarm-package` from each field tag can install a protocol-bumping bundle.
**Do not use this for a bump that test has not been shown to survive.**

The base URL **must include the `/vX.Y.Z/` segment**. Omitting it is how 0.8.7
published a manifest pointing at a 404 and stranded a developer on a download
error.

The manifest states what is offered **now** — it rewrites the whole document from
the tarballs named, so a release is withdrawn by ceasing to list it.

## 10. Fetch the artifact through the manifest's own URL

```bash
URL=$(python3 -c "import json;print(json.load(open('releases.json'))['payload']['releases'][0]['artifact_url'])")
curl -sSL -o /tmp/dl.tar.gz -w '%{http_code}\n' "$URL"
sha256sum /tmp/dl.tar.gz
python3 -c "import json;print(json.load(open('releases.json'))['payload']['releases'][0]['artifact_sha256'])"
```

Expect 200 and identical hashes. This is the step that catches a manifest
pointing somewhere wrong, and it is the reason 0.8.7 cannot happen again.

## 11. Publish

```bash
git add releases.json && git commit -m "release: offer X.Y.Z to Hives" && git push origin main
gh api repos/miopea/swarm-next/contents/releases.json --jq '.content' | base64 -d | head -8
```

Publishing the manifest is what makes a release real. A tarball on GitHub and
absent from the manifest is offered to nobody.

## 12. Propagation — published is not live

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
