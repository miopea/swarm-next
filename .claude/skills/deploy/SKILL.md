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

## 2. Bump

Edit `version` in the root `Cargo.toml` (eight crates share it), then let cargo
refresh the lock:

```bash
sed -i '15s/^version = "0.8.16"/version = "0.8.17"/' Cargo.toml
CARGO_INCREMENTAL=0 cargo check --workspace --quiet
```

Ask the operator for the version if it is anything other than a patch bump.
Patch is right almost always, and wrong exactly when it matters.

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
