# Checks that measure the wrong thing

Filed after a single night in which one worker shipped four regressions the
operator found by using the product, then made a fifth instance of the same
mistake *while writing the task about the first four*.

Every one had a test. Every test ran. Every test passed.

## The first shape: narrower than the claim

None of these was a wrong assertion. In all five the assertion was **true**. The
gap was between the thing asserted and the thing claimed:

| The claim | What the check actually measured |
| --- | --- |
| the invariant holds | one call site that implements it |
| the row is reduced at phone width | one branch of the conditional that renders it |
| the fix is live | a working tree that was not the one that shipped |
| the refusal is accurate | a predicate, not the sentence it prints |
| 49 tasks are unverified | a join admitting rows the claim excluded |
| exactly one control in the dialog is primary | one row of the dialog, not the dialog |

Asserting harder closes none of them. That is the useful thing to notice: the
instinct after a miss like this is *more tests*, and more tests of the same
subject would have passed too.

## The second shape: a check that could not have returned the other answer

The six above are all NARROWER than their claim — one call site, one branch,
one predicate, one working tree, one join, one row of a dialog. Each measures
something real, just less than was said. They are widened.

A second shape is worse and is not in that table. The check carries no
information about the claim at all: it would have printed the same thing in
either world. There is nothing to widen, because nothing was measured. It has
to be discarded and replaced.

**The test that separates them.** Ask what the check would have printed if the
opposite were true. If the answer is "the same thing", it is this shape.

### The worked example, because nothing about it looks careless

On 2026-08-31 Queen was asked whether this Hive was serving a fixed dialog. She
read the uptimes of its `swarm-terminal-host` processes — 1.7 to 17.7 hours at
that moment, all comfortably older than the release carrying the fix — and
reported that the Hive was still serving the old one. Confidently, to the
operator.

(Those figures are a record of one reading, not a standing fact: the same
command a few hours later returns 1.4 hours to 3.6 days. Do not check them.
They were never the point, which is rather the argument.)

The terminal host serves PTYs. It does not serve the web bundle; the API does.
And host uptime is long **by design**, because the host deliberately survives an
API reload so worker terminals are not killed. So the number is large whether or
not the fix is being served, and the probe could not have said otherwise.

What settles it is the served artefact:

    GET :8766 -> /assets/index-*.js   contains "Checking where this goes"

Read from the live endpoint rather than the tarball. It was there; the Hive was
serving the fix.

The reasoning around that probe was careful, the check was reasonable, and it
was reported as evidence. That is why it is the example. Her own words for it
are better than "measured the wrong thing": *a check whose output was fixed in
advance*.

### The document already sorted them and never said so

Every entry in the historical list above is this shape, not the table's. The
colourised `grep` matched nothing whether or not errors existed. The
line-addressed `sed` changed nothing whether or not a bump was needed. The
reconcile compared a symlink against itself, which is equal in every world. The
schema tests ran against empty databases, where no row can violate a
constraint. `systemctl show` on a unit that does not exist returns empty
defaults rather than an error.

Six in the table, narrower than the claim. Five in the paragraph, incapable of
failing. The difference was visible in the document before it was named.

### The rest of 2026-08-31

Four more, all in the same day and all found by ablation or by a person looking
at a screen rather than by another test:

| The check | Why it could not have failed |
| --- | --- |
| an ablation filtered to a test name that did not exist | zero tests matched, so it printed `ok` whether or not the guard was removed |
| `expect(tagName).toBe("STRONG")` | the element is `STRONG` whether or not it renders bold, which was the claim |
| "blocks controls while disconnected", asserting a button disabled | that render supplied no handler, so the button was disabled for an unrelated reason |
| a find-and-replace whose `\d+` could not match `1_000` | zero substitutions, exit zero — identical to nothing needing changing |

The last is already in this document as *an edit that matches nothing reports
success*, written before the category had a name. It belongs to both.

## The third shape: a check taught the wrong answer

The two shapes above are found by the same question — *what would make this go
red?* One answers "less than I claimed"; the other answers "nothing".

This one answers the question perfectly well and is still wrong.

```rust
assert_eq!(extension_of(store.save(DOCX, b"PK\x03\x04document").await.unwrap()), "xlsx");
```

A Word document was being stored as `.xlsx`, and the test asserted that it was.
The mapping was wrong, the test agreed with it, and the suite was green while
every Word document on every Hive was written under a false extension. Nothing
could report it, because the only thing that would have reported it had been
taught to expect the defect.

**Its sign is inverted rather than its output constant.** It has a clear red
condition — it goes red the moment the code becomes correct, which is exactly
what happened when the arm was fixed. It is not narrower than its claim and it
is not incapable of failing. It is a capable check pointed at the wrong value.

So the question that catches the other two does not catch this one. *What would
make this red?* has a crisp answer: storing a `.docx` as `docx`. That answer
sounds like a passing grade and is the defect.

### The heuristic that actually works, because "re-derive it" will not be run

The honest general rule is *check that the asserted value is the DESIRED value,
not merely the OBSERVED one* — a test written by running the code and recording
what came out encodes behaviour, not intent. True, and nobody re-derives an
expectation once a test is green, so it is not a check anyone will actually run.

Something sharper is available whenever the subject is a **mapping**:

> **Test the round trip, not the individual arms.** A round trip cannot encode a
> colliding wrong answer, because the collision breaks the return leg.

Here the write side mapped three distinct media types — XLSX, DOCX, PPTX — onto
one extension. A per-arm test can assert each of those three individually and
be green. A round trip cannot: `DOCX → "xlsx" → XLSX` does not come back to
`DOCX`, so the test fails without anyone re-deriving anything, and it fails
pointing straight at the collision.

That is mechanical rather than a discipline, and it generalises past this bug.
`attachments.rs` now carries *every extension the store writes can be read
back*, which is the same idea one level up: a write arm with no read arm stores
files that can never be fetched, and a suite complete on one side of a round
trip is not complete.

Two cheaper reading habits fall out of the same observation:

- **A collision in a mapping deserves a sentence.** Three inputs to one output
  is either deliberate or a bug, and if no comment says which, assume nobody
  decided.
- **Ask where an expected value came from.** If it could have been produced by
  running the code, it probably was.

### The cousin: a comment right about its premise, blind to its conclusion

The same function carried an honest doc comment. It said the signature check
"proves the file is a zip and not that it is a workbook" — which is exactly
true, and is the strongest claim the format allows.

The next line then named a workbook regardless.

The comment was not wrong. It described the check's limitation accurately and
did not notice that the code immediately exceeded it. This is the same failure
as the test, viewed from the other side: the test encoded the wrong
expectation, and the comment documented the right limitation without applying
it. Both were written by someone paying attention; neither closed the loop
between what was measured and what was then said.

## Why this repository keeps producing it

It has produced it for as long as anyone has kept records — the colourised
`grep` that matched nothing and reported success for hours; the line-addressed
`sed` that changed nothing; the reconcile that compared a symlink against
itself; the schema tests that passed against empty databases; the `systemctl
show` on a unit that does not exist, which returns empty defaults instead of an
error.

The worker that caused the four above **had read all of those** and produced
four more the same night. So this is not a knowledge problem, and a warning is
not a fix. It catches people who are actively looking for it.

## What actually helps

**Name the subject out loud before writing the check.** Not "does this pass" but
"if the thing I am claiming were false, would this go red?" The five failures
above all survive that question being asked properly.

**Prefer a check whose subject cannot drift from the claim.** `9bceee4` is the
model: rather than testing the build harder, it made the build *refuse to lie
about what it contains* — fingerprinting the tree before and after so a bundle
built over a moving checkout is never installed. The claim and the subject
became the same object.

**Count the places a rule can be disobeyed, not the places it is obeyed.**
`web/src/terminal/geometryInvariant.test.ts` asserts the mutating `fit()` has
exactly one caller. A behaviour test can only cover a path someone thought of;
this one fails when a path is *added*. It found a second caller the moment it
was written — a caller that was correct, and would not have been told when the
rule changed.

**A fixture that renders one arm of a conditional cannot fail for the other.**
Both branch-shaped defects here were fixture defects, not assertion defects. If
the code says `A ? x : y`, the fixture owes you both.

**An edit that matches nothing reports success.** Three times in one session a
find-and-replace silently did nothing — usually because `cargo fmt` had reflowed
the target after it was written — and each time the tool said it had worked. Once
this happened *while ablating a guard*: the ablation modified an unmodified file,
the test stayed green, and the conclusion was that the guard did not bite. It did.
Assert the match count before writing (`assert s.count(old) == 1`); the edits that
went wrong were the ones that skipped it.

**An ablation is the only evidence a check works.** Break the mechanism, watch
the check go red, restore it. A check that has never failed has told you
nothing — and this is where the fifth instance came from: an ablation that
passed against a fixture covering one branch.

## What this does not claim

There is no mechanism here that catches the class. There are two that catch two
instances of it, and both state their own limits: the reload guard hashes a tree
and cannot see a build reading anything outside it; the call-site guard counts
text and cannot see an alias, a destructured method or a dynamic property.

Deliberately no new ceremony. The operator has already said verification costs
too much manual clicking, and a process tax that does not catch this class would
make the product worse to work on while leaving the defect in place.
