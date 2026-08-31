# Operator authentication

Status: **Scoped 2026-08-23.** Token rotation first, then passkeys.

## What prompted it

The operator was locked out for an hour. The cause was mundane — a stale token
in a password manager — but the hour was not, and nothing in the product helped:

- **The error could not distinguish the two failures.** `auth.rs` returns "a
  valid operator session is required" both when the bearer does not match and
  when the session cookie does not, so a wrong token and a dropped cookie read
  identically.
- **There was no way to change the token.** It is an immutable `Arc<str>` read
  from `swarm.env` at process start, so rotating it means editing a file and
  restarting a service.
- **A wrong guess cost real time.** The token is 64 characters of hex that has
  to be copied between a file, a password manager, and a phone.

That last point is the case for passkeys, and the operator made it: this is one
class of failure, and it is the copy-paste.

## Decided

**Localhost needs no token.** The operator's ruling: "localhost is not public,
it shouldn't need any tokens. Anything NOT localhost would be a passkey or a
fallback to token."

**With one guard, recorded because "not public" is not "not reachable".** A page
on any website can make requests to `http://127.0.0.1:8766` from the operator's
browser. Cross-origin rules stop it reading most responses and a JSON mutation
needs a preflight it cannot satisfy, but that is the browser being careful
rather than Swarm being safe, and DNS rebinding defeats an origin check on its
own. On this installation localhost is also forwarded from Windows into WSL, so
"local" is a larger set than it sounds.

So unauthenticated access requires **both**: a `Host` that is literally loopback,
and no foreign `Origin`. A browser acting for another site fails the second test
even when it passes the first. Convenience kept, drive-by closed.

**Anything else needs a passkey, or the token as fallback.** The token remains
the recovery path — a passkey lives on one device, and losing that device must
not lose the Hive.

**Rotation signs out everywhere.** The session cookie is derived from the token,
so changing it invalidates every browser session at once. That is the point of
rotating rather than a side effect, and the button says so before it acts.

## Passkeys are per domain, and that shapes the feature

A WebAuthn credential is bound to one relying-party ID. This Hive answers on
`localhost:8766` and on `swarm2.bfgsolutions.net`, and a passkey registered for
one **will not work on the other**.

With localhost unauthenticated that mostly resolves itself: passkeys exist for
the public domain, and localhost never asks. Settings lists registered passkeys
so the operator can see what exists and remove one, because a credential you
cannot enumerate is one you cannot revoke.

## Settled, 2026-08-31

**Recovery is a `swarmctl` command on the box itself.** The operator chose this
over a one-time code and over keeping the token as a permanent fallback, and the
argument that decided it is that it grants nothing new: the Hive runs on the
operator's own machine, and anyone with shell access can already read
`swarm.sqlite3` outright. A local recovery command is a better door to a room
they already own — which is the same test `secrets.md` applies to impersonation,
reached from the opposite direction.

It also invents no new secret. A recovery code has to be stored somewhere safe,
and the event it guards against — losing a device — is the event most likely to
lose the note it was written on.

Still open, and deliberately smaller than they look:

- **Where the credential lives.** A new table, or the operator record. No
  consequence outside persistence; decide it while writing the migration.
- **Whether a passkey replaces the session cookie or mints one.** Minting keeps
  one session model and less new machinery, which is the standing preference
  unless something argues otherwise.

## Not in scope

Per-person identity. One Hive belongs to one person and Swarm records no
per-operator history — settled 2026-08-22 and unchanged by this. Passkeys here
are a better key to the same door, not a user system.
