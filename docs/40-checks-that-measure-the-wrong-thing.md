# Checks that measure the wrong thing

Filed after a single night in which one worker shipped four regressions the
operator found by using the product, then made a fifth instance of the same
mistake *while writing the task about the first four*.

Every one had a test. Every test ran. Every test passed.

## The shape

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

## The other shape: a check that could not have returned the other answer

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
