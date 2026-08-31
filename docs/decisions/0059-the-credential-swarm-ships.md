# ADR 0059: The credential Swarm ships so a fresh install can report a bug

## Status

Accepted, 2026-08-31, on the operator's ruling recorded as decision
`01a05973-0855-71b0-8708-d57dfbecf86d`: **"Ship a token scoped to issues-write
on the Swarm repo only."**

Their summary of the trade, in their own words on the decision:

> "embed it in the install" means shipping a secret in a distributed binary,
> which is extractable by anyone holding the binary. That is workable if scoped
> tightly — this is the choice, with the scoping stated so it is deliberate.

## What was wrong

Anonymous feedback shipped in 1.0.0 as a first-class feature. It worked only
when the person installing had personally obtained a GitHub token and written it
into `swarm.env`. Two things were wrong at once, and the operator named both:

> "so you are telling me that devs need to install settings to make it work?
> that is stupid. Why would we not embed that in the install since it is
> critical for anon submitting. instead you want me to give out a token?!? that
> is stupid."

1. **Every installer had to do credential setup before a core feature worked at
   all**, and nothing in the product said so. On a Hive with no credential the
   dialog offered only "Save to this Hive" — which is a true description of what
   happens and reads as a *choice*, an install deliberately keeping things
   local. It was not. It was an install that could not reach the project. The
   two were indistinguishable on screen, which is why this survived a release.

2. **Strangers' reports went out under the operator's own account.** The
   anonymous path files on "this Hive's credential", and that credential belongs
   to a person. Every anonymous issue was attributable to them, and revoking it
   to stop abuse would also have stopped their own filing.

The deeper problem was that two coherent designs had both been built and nothing
chose between them. `SWARM_GITHUB_REPOSITORY` assumes each operator files into
their own repository, which is right for a Hive whose owner triages their own
issues. The anonymous path assumes feedback about Swarm reaches Swarm's
maintainers from someone with no relationship to the repo. Both shipped.

## The decision

Swarm ships its own credential for its own repository. A fresh install files
feedback about Swarm with no setup. `SWARM_GITHUB_REPOSITORY` and
`SWARM_GITHUB_TOKEN` remain, and take precedence, for operators who want their
own destination.

### The scoping is the decision, not a detail of it

A **fine-grained** token, **`issues: write`** on **one repository**, and no
other permission. Not a classic PAT, not `repo` scope, not "issues plus a bit
more because it was easier".

The entire case for this shape rests on the blast radius being *someone can open
issues in the Swarm repository*. Widen the scope and the operator agreed to
something else. **If a change appears to need a broader permission, that is a
question to take back to them, not a detail to settle in a commit.**

### It is not hidden, and nothing pretends it is

The token is a string literal in a distributed binary. `strings` finds it.
Anyone holding the artefact holds the credential.

Nothing obfuscates or encodes it. An encoding would imply a protection that does
not exist, and its only real effect would be to mislead the next reader into
trusting the artefact more than they should. A reader of the source should be
able to see exactly what ships and reach the same conclusion the operator did.

### Rotation is a release

Revoke at GitHub, put the new value in the 1Password item, build again. That is
the floor and it is inherent to shipping a secret in an artefact.

What was avoidable is making it worse, so the value is named **once**
(`crates/swarm-api/src/bundled_feedback.rs`) and reaches the build through a
single environment variable, `SWARM_BUNDLED_FEEDBACK_TOKEN`. Nothing else in the
tree holds a copy.

Cargo re-runs the compile when that variable's value changes — **measured, not
assumed**: same value, 0 recompiles; changed value, 1. Had that not held,
rotating the token would have silently shipped the old one, which is the failure
mode this whole codebase keeps finding in other costumes.

## Precedence, and why half a credential is not rescued

`bundled_feedback::feedback_destination` is a pure function with tests, rather
than a `match` inside `main.rs`, because the precedence *is* the acceptance
criterion and a rule living only in a binary's private function cannot be
asserted by anything.

| `SWARM_GITHUB_REPOSITORY` | `SWARM_GITHUB_TOKEN` | Destination |
| --- | --- | --- |
| set | set | The operator's own. Theirs always wins. |
| set | unset | **Refused, and named.** |
| unset | set | **Refused, and named.** |
| unset | unset | The shipped credential, into the repository it is scoped to. |

The half-credential rows are the ones worth explaining. It would have been easy
to let them fall through to the shipped credential, and it would have been
wrong: an operator who set `SWARM_GITHUB_REPOSITORY` is telling us where their
reports go, and filing them into the Swarm repository instead would send their
words somewhere they never asked for. Declining to file is the better failure.

This preserves the asymmetry that was already in `main.rs` and was right: half a
credential is a mistake worth naming; **no** credential is a legitimate
configuration. What changed is that "no credential" is now rare rather than
ordinary — it is a `cargo build`, not a fresh install.

## The silent degradation, fixed separately

This is a distinct defect from where the credential comes from, and it is fixed
regardless of the shape chosen. A Hive that cannot file now says so in the
dialog, before the report is written rather than after.

Readiness is also fetched when the dialog **opens** rather than on the first
keystroke. It used to be requested only from `markChanged`, so a reporter was
told where their words were going only once they had already written them.

## What this does not fix

- An operator who configures their own repository and token still has anonymous
  reports on their Hive filed under their credential. That is their own
  repository and their own explicit choice, and it is the case the environment
  variables exist to serve.
- A build with no bundled token still files nowhere. That is every `cargo
  build`, and the dialog now says so.
