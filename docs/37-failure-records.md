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
- **Redaction is not implemented.** `detail` is a tool's own stderr and could in
  principle carry a path or a value. Nothing scrubs it. Bounding it to one line
  is not the same as making it safe, and this is the open risk in the design.
