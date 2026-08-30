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

Asserting harder closes none of them. That is the useful thing to notice: the
instinct after a miss like this is *more tests*, and more tests of the same
subject would have passed too.

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
