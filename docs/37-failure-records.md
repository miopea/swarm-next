# Failure records — one shape for every failure

Filed after the operator, 2026-08-28: *"two issues with an update, 1. they keep
failing and 2. it is taking a lot of work to find errors"*, then *"We need to
standardize our error trapping and recording system so this is much easier to
fix and all errors and issues are faster to fix."*

The second sentence is the cause of the first. Failures were not more frequent
than the work warranted; they were **indistinguishable from each other**, so
every one cost a full investigation and the whole thing read as unreliable.

## The measurement

`packaging/linux/swarm-package` has **87 `die()` calls. Six of them reached a
status file.** The other 81 spoke only to the journal.

That is why a reload whose build succeeded and whose *install* was refused for a
protocol mismatch reported "The development working copy did not compile" — a
confident false claim that sent the reader to the wrong file — and why a reload
that genuinely did not compile reported exactly the same sentence.

## The four fields

A failure carries, everywhere, in one shape — the four questions an operator
asks, in the order they ask them:

| Field | Answers |
| --- | --- |
| `step` | which part failed, named the way the operator thinks of it |
| `detail` | what the failing thing itself said — its words, not our paraphrase |
| `changed` | what was left different: `nothing`, `partial`, `unknown` |
| `next` | the command that moves forward, when one exists |

## Where it lives, and why there

**The status file, written at the exit.** Three shapes were available — the
status files, `ApiError`, and tracing — and the status file wins for one reason:
it is the only one already read by a surface the operator looks at without being
told to.

**Recorded in `on_exit`, not at each `die()`.** Everything ends there: `die()`
exits, and `set -e` aborts do too. One writer covers the paths nobody remembered
to instrument as well as the ones they did. A helper each `die()` had to call
would only ever cover the calls someone updated — which is how six out of 87
happened in the first place.

## Rules

**`changed` is maintained by the code that changes things.** Not concluded
afterwards, not defaulted. The two rollback branches in `on_exit` are the only
code that knows a rollback ran, so they are what set it; `apply_release` reads
the installed `VERSION` back rather than trusting an exit status.

A surface saying "nothing was changed" when it never looked is the same defect
as a reload saying a build did not compile when it compiled. **Both are worse
than saying nothing, because a reader who believes them stops looking.**

**`unknown` is a legitimate value and must stay available.** `apply_release`
delegates to a bundle that runs its own rollback in its own process; from here
that outcome is genuinely unobservable. Naming that beats guessing.

**The detail is one bounded line.** These files are parsed as `key=value`: a
detail carrying a newline becomes a key of its own and silently corrupts every
field after it. `last_error()` bounds it to 300 characters and strips newlines.

**`last_error()` takes the FIRST strong match, not the last.** Cargo prints the
cause first and a summary last — `error[E0433]: failed to resolve` then `error:
could not compile due to 1 previous error`. Taking the last match hands the
operator the summary, which names no file and no reason and is exactly the
uninformative sentence this whole mechanism exists to replace.

**A missing field renders nothing, never the old confident sentence.** A status
file written before 0.8.20 carries no `changed`, and the card says nothing about
what was left behind rather than reverting to the assumption it used to state.

## Where it is applied

| Path | Steps it names |
| --- | --- |
| Development reload | `build`, `install`, `protocol-change` |
| Release install | `accept`, `update`, `migrate-protocol`, with `changed` read back |
| Pre-update backup | `backup`, and the `cp` command that moves forward |

## What is not done

- **`ApiError` and tracing are untouched.** Converging them was not attempted;
  this covers the install and reload paths, which is where the operator's time
  went. The four-field shape is the candidate for anything that follows.
- **`next` is carried but rendered by only one surface.** The record has the
  field and the backup path writes prose; a card that renders `next` uniformly
  does not exist yet.
- **`detail` is not scrubbed, and deliberately so.** See the section below: the
  credential-bearing command's output is excluded at the source instead. No
  pattern matching is applied to what does get through.


## What can reach `detail`, enumerated

The acceptance for this was "enumerated from the code rather than imagined", so
this is the list, not a description of one.

`fail_detail` is set in exactly four places:

| Source | What it carries |
| --- | --- |
| `die()` fallback | our own message — every one is a literal we wrote |
| `build-development-release.sh` stderr | cargo, vite, git. Handles no credential. |
| nested `swarm-package update` stderr | its own messages, plus any unredirected child |
| nested `swarm-package <apply>` stderr | the same |

The last two are the ones that matter, because a nested `swarm-package` runs
`create_update_backup`, which calls the API **with the operator token in an
`Authorization` header**. Before this change, curl's stderr flowed straight into
the captured stream and onto a rendered card.

### What was measured, and why measuring was not enough

curl 8.5.0 does not echo config-file contents for our shape on any failure
tried — connection refused, HTTP 4xx, unreadable config, malformed config,
unwritable `--output`. It echoes an argument only when the secret is written
*as an option*, which is not how it is written here.

**That is one version of one tool, and the guarantee has to hold for the next
one.** So the measurement is recorded as background, not relied on.

### The rule

`authenticated_curl()` is the only caller that passes `--config`. Its stderr
goes to a private file that is deleted; what reaches the operator is curl's
**exit code**, translated through curl's own documented table:

| Exit | Rendered as |
| --- | --- |
| 7 | the API is not reachable |
| 22 | the API refused the request |
| 28 | the API did not answer in time |
| 23, 26 | a local file could not be read or written |

Nothing that function emits is derived from curl's output. That is a property of
this code, not a property of curl — which is the whole point.

The unauthenticated health poll in `wait_for_health` still reports normally. It
carries no credential, and narrowing it would cost diagnosis for nothing.

### Not a denylist

Nothing scans `detail` for things that look like secrets. A pattern that misses
one shape reads as protection and is not. The guarantee here is that the
credential-bearing command's bytes never enter the channel — which is testable
by ablation, and is tested: the harness's curl stub speaks a token back
deliberately, and the assertion is that it does not appear in the captured
stream. No real curl was observed doing that; the test asserts the channel is
closed rather than asserting curl's manners.

### Where the record can travel

Both rendering endpoints — `/api/v1/runtime/development` and
`/api/v1/runtime/release` — require the operator token and return 401 without
it. Nothing forwards `detail` to a notification, an email, or an off-box log.

**But the control room can be published through the tunnel**, so "it is only
ever loopback" is not a property this design may assume. That is the specific
reason the 2026-08-27 token-in-transcript ruling does not transfer: that one
turned on the API being loopback-only and both files being 0600 under one user.
A rendered card has neither guarantee.

### Still open

`build-development-release.sh` output is captured verbatim. It handles no
credential today, and a build script that started printing one would reach a
card. Nothing enforces that — it is an observation about the current script,
not a mechanism.
