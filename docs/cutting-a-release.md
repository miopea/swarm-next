# Cutting a release

What has to happen for a Hive somewhere else to learn a release exists, trust
it, and install it. The design and its reasoning are in
[ADR 0050](decisions/0050-discovering-and-fetching-a-release.md); this is the
procedure.

## The signing key

One Ed25519 keypair. The public half lives in `packaging/release-verifying-key`
and is compiled into every build; the private half lives in 1Password and
touches nothing else.

This is the part worth being careful about. **A verifying key fetched from the
same place as the manifest verifies nothing** — whoever can replace one can
replace the other — so it is compiled in, and the trust chain is that you
trusted the build you installed.

The consequence is that **rotating the key strands every existing install**. A
new key cannot be announced through a channel the old key protects, so every
Hive would need updating by hand. That is the correct failure rather than a
flaw: silent key replacement is exactly the attack being refused. Rotate rarely,
and announce it.

It is stored as **Swarm release signing key** in the `BFG` vault of the personal
1Password account (`my`), in a concealed `credential` field.

Creating one, which has been done and should not be done again without meaning
to:

```
cargo build --release -p swarm-release-tool
./target/release/swarm-release keygen /run/user/$(id -u)/swarm-signing.key
```

It prints the public key and nothing else — the private half is never printed,
logged, or returned. Put the public key in `packaging/release-verifying-key`,
put the private key in 1Password, and delete the file.

Verify the stored copy before deleting the original, by digest rather than by
looking at it:

```
diff <(op read --account my "op://BFG/Swarm release signing key/credential" | tr -d '\r\n' | sha256sum) \
     <(tr -d '\r\n' < /run/user/$(id -u)/swarm-signing.key | sha256sum)
```

A key nobody read back is a key nobody has stored.

## The feedback credential

A second, unrelated secret rides along in the build, for a different reason and
with a very different risk. The signing key's private half **never** leaves
1Password. This one is deliberately **shipped inside the artefact**, so that
someone installing Swarm can report a bug about Swarm without first obtaining a
GitHub token of their own. See
[ADR 0059](decisions/0059-the-credential-swarm-ships.md) for the reasoning and
the operator's ruling.

It is stored as **Swarm feedback token** in the `BFG` vault, in a concealed
`credential` field, matching the signing key's shape. Override the location for
a different vault with `SWARM_FEEDBACK_TOKEN_REFERENCE`.

**The scope is the whole basis of this being acceptable.** Create it as a
**fine-grained personal access token**, with:

- **Repository access:** only `miopea/swarm-next`.
- **Permissions:** `Issues: Read and write`. Nothing else. No `Contents`, no
  `Pull requests`, no `Metadata` beyond what GitHub forces.

Anyone holding a release holds this token. What that buys them must stay
"they can open issues on the Swarm repo".

`build-release.sh` reads it and passes it to `cargo` as
`SWARM_BUNDLED_FEEDBACK_TOKEN`. It prints the credential's **length** and never
any part of its value, and **do not run the script under `set -x`**, which would
print every expansion including this one.

Building without it is legitimate and loud rather than silent — a local package
for testing has no business carrying the project's credential:

```
swarm-package: 'op' is not on PATH; building without a bundled feedback token
swarm-package: bundling a feedback token (93 characters)
```

Read that line. A release that prints the first message files nowhere for anyone
who installs it, and the Hive will say so in its feedback dialog rather than
quietly offering to save locally.

To rotate: revoke at GitHub, create a replacement with the same scope, update
the 1Password item, and cut a release. Cargo recompiles when the value changes,
so the new token really does reach the artefact — that is checked rather than
assumed, and there is a test for the embedding path:

```
SWARM_BUNDLED_FEEDBACK_TOKEN=placeholder cargo test -p swarm-api bundled_feedback
```

## Building the release

`build-release.sh` refuses an untagged commit, deliberately: a release version
has to come from a tag or two releases cannot be ordered.

```
git tag -a v0.2.0 -m "Swarm 0.2.0"
./packaging/linux/build-release.sh
```

That leaves a tarball under `dist/`.

## Publishing the manifest

The manifest states what is offered **now**. A release is withdrawn by ceasing
to list it, so name every tarball that should still be on offer — this rewrites
the document rather than appending to it.

The key lives in 1Password and should exist on disk only for the length of this
command. `/run/user/$UID` is tmpfs, so it never reaches a disk at all.

```
eval "$(op-login)"
umask 077
op read --account my "op://BFG/Swarm release signing key/credential" \
  > /run/user/$(id -u)/swarm-signing.key

./packaging/linux/publish-release-manifest.sh \
  /run/user/$(id -u)/swarm-signing.key \
  https://github.com/miopea/swarm-next/releases/download/v0.2.0 \
  dist/swarm-0.2.0-linux-x86_64.tar.gz > releases.json

rm -f /run/user/$(id -u)/swarm-signing.key
```

Then upload the tarball to the release, and publish `releases.json` at the URL
Hives read. Nothing in `docs/install.md` changes: `install.sh` reads the same
manifest and takes whatever it offers, so the published install command is
already pointing at this release the moment the manifest lands.

Publishing the manifest is therefore what makes a release real. A tarball
uploaded to GitHub but absent from the manifest is offered to nobody — not to
running Hives, and not to fresh installs either, which is what makes withdrawing
a release by removing it from the manifest actually work — by default
`https://raw.githubusercontent.com/miopea/swarm-next/main/releases.json`,
overridable per Hive with `SWARM_RELEASE_MANIFEST_URL`.

The script reads each bundle's own `VERSION`, `PROTOCOL` and
`WORKER_ENGINE_BUILD_ID` rather than parsing filenames, refuses a development
build, and refuses a base URL that is not https.

## What a Hive does with it

1. Fetches the manifest, if the operator turned checking on. Sends nothing.
2. Checks the signature against the compiled key **before** reading any URL
   inside it.
3. Ignores any release whose `PROTOCOL` differs from its own, so an operator is
   never the person who discovers an incompatibility.
4. Offers nothing at all to a Hive built from a working copy.
5. On download, checks the artifact's digest against the signed manifest before
   unpacking. A mismatch is discarded, not installed and rolled back.
6. On install, writes a request naming the verified directory;
   `swarm-release-apply.path` runs `swarm-package apply-release`, which refuses
   anything outside the download root and re-verifies the bundle's own
   `SHA256SUMS`.

## Checking it worked

Do not trust the publish step on its own — verify the document the way a Hive
will, from a machine that is not the one that signed it:

```
curl -fsSL https://raw.githubusercontent.com/miopea/swarm-next/main/releases.json | head
```

and confirm a Hive with checking on reports the new version under
**Settings → System** after **Check now**. A manifest nobody verified is a
manifest nobody has tested.
