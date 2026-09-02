"""Find tests that cannot fail -- the stratified sweep from task 01a0635f.

A test that cannot fail passes on day one, passes forever, and reads in CI
exactly like a guard that works. Three were found in one day in September 2026,
each by someone who was about to DEPEND on one and checked it, never by anyone
looking. That is why the class had never been counted: it is always repaired as
collateral inside another fix and never filed as a defect of its own.

⚠️ NO TEXT SCAN FINDS THIS CLASS IN GENERAL. All three known instances had a
perfectly ordinary-looking assert; what was wrong was what the assert was
pointed AT. A census of tests with no failure path at all -- no assertion, no
unwrap, no expect, no should_panic -- returns ZERO across all 983 tests here.
The trivially vacuous shape does not exist in this workspace, so the interesting
cases are all invisible to text.

⚠️ AND A UNIFORM RANDOM SAMPLE IS THE WRONG INSTRUMENT. Three instances in ~983
tests is a base rate near 0.3%; a random 30 finds nothing with probability ~0.91
and would be reported as "none found", which reads like evidence of absence.
Stratify by mechanism instead. This script is one stratum.

ONLY ABLATION DECIDES. Everything below is a candidate list to ablate, never a
verdict -- when this was first run, 9 of its 10 hits were false positives, and
the tenth was real and had been green for months.

THE STRATUM: assertions that are VACUOUSLY TRUE on an empty collection.

Instance 2 of the three known cases was this shape -- the schema ceiling tests
ran against empty databases, so what they guarded had no rows to fail on. It is
the one mechanism of the three that leaves a textual signature, because the
vacuous truth is in the CONSTRUCT rather than in the matcher:

    for x in xs { assert!(...) }      passes when xs is empty
    assert!(xs.iter().all(...))       passes when xs is empty
    assert!(xs.iter().any(...))       CANNOT pass when empty, so it is sound
    assert_eq!(xs.len(), 0)           an assertion ABOUT emptiness, not victim to it

So the finding is: a test that only ever asserts inside such a construct, and
never establishes the collection is non-empty, is green on an empty fixture
regardless of what the code does.

⚠️ Fixtures first, and the tool reports nothing if it fails one.
"""
import re, pathlib, json, sys

TEST_ATTR = re.compile(r'#\[(?:tokio::)?test\b[^\]]*\]')
FN = re.compile(r'\bfn\s+([A-Za-z0-9_]+)\s*\(')
ASSERTION = re.compile(r'(assert!|assert_eq!|assert_ne!)')
# Guards that establish the collection is not empty, so the vacuous case is closed.
NONEMPTY_GUARD = re.compile(
    r'(assert_eq!\s*\(\s*[^,]*\.len\(\)|assert!\s*\(\s*![^)]*is_empty\(\)|'
    r'assert!\s*\([^)]*\.len\(\)\s*[=>]|assert_ne!\s*\(\s*[^,]*\.len\(\)|'
    r'\.any\(|assert_eq!\s*\(\s*[^,]*\.count\(\))'
)
FOR_LOOP = re.compile(r'\bfor\s+[^\n{]+\bin\b[^\n{]*\{')
ALL_IN_ASSERT = re.compile(r'assert!\s*\([^;]*\.all\(')

def test_bodies(src):
    for m in TEST_ATTR.finditer(src):
        fm = FN.search(src[m.end(): m.end() + 400])
        if not fm:
            continue
        i = src.find('{', m.end() + fm.end())
        if i < 0:
            continue
        depth, j, in_str, esc = 0, i, False, False
        while j < len(src):
            c = src[j]
            if esc: esc = False
            elif c == '\\': esc = True
            elif in_str:
                if c == '"': in_str = False
            elif c == '"': in_str = True
            elif c == '{': depth += 1
            elif c == '}':
                depth -= 1
                if depth == 0: break
            j += 1
        yield fm.group(1), src[m.start(): j + 1]

def assertions_only_inside_a_vacuous_construct(body):
    """True when every assertion sits in a for-loop or an .all(), and nothing
    establishes the collection is non-empty."""
    if not ASSERTION.search(body):
        return False
    if NONEMPTY_GUARD.search(body):
        return False
    has_loop = bool(FOR_LOOP.search(body))
    has_all = bool(ALL_IN_ASSERT.search(body))
    if not (has_loop or has_all):
        return False
    if has_all and not has_loop:
        return True
    # Every assertion must be inside a for-loop body. Walk braces from each
    # `for` and check no assertion sits outside all of them.
    covered = []
    for fm in FOR_LOOP.finditer(body):
        i = body.index('{', fm.start())
        depth, j = 0, i
        while j < len(body):
            if body[j] == '{': depth += 1
            elif body[j] == '}':
                depth -= 1
                if depth == 0: break
            j += 1
        covered.append((i, j))
    for am in ASSERTION.finditer(body):
        if not any(a <= am.start() <= b for a, b in covered):
            return False
    return True

FIXTURES = [
    ("plain assert is not vacuous",
     '#[test]\nfn t() { assert_eq!(f(), 3); }', False),
    ("assert only inside a for loop IS vacuous",
     '#[test]\nfn t() { let xs = go(); for x in xs { assert!(x > 0); } }', True),
    ("for loop guarded by a length check is not",
     '#[test]\nfn t() { let xs = go(); assert_eq!(xs.len(), 3); for x in xs { assert!(x > 0); } }', False),
    ("assert!(all) is vacuous",
     '#[test]\nfn t() { let xs = go(); assert!(xs.iter().all(|x| *x > 0)); }', True),
    ("assert!(any) is not, it fails on empty",
     '#[test]\nfn t() { let xs = go(); assert!(xs.iter().any(|x| *x > 0)); }', False),
    ("an assertion outside the loop rescues it",
     '#[test]\nfn t() { let xs = go(); assert!(!xs.is_empty()); for x in xs { assert!(x > 0); } }', False),
    ("no assertion at all is a different finding",
     '#[test]\nfn t() { let xs = go(); for x in xs { let _ = x; } }', False),
]
bad = []
for name, src, expected in FIXTURES:
    bodies = list(test_bodies(src))
    if len(bodies) != 1:
        bad.append(f"{name}: extracted {len(bodies)}")
        continue
    got = assertions_only_inside_a_vacuous_construct(bodies[0][1])
    if got != expected:
        bad.append(f"{name}: got {got}, expected {expected}")
if bad:
    print("TOOL FAILED ITS OWN FIXTURES -- reporting nothing:", file=sys.stderr)
    for b in bad: print("  " + b, file=sys.stderr)
    sys.exit(1)
print(f"tool fixtures: {len(FIXTURES)}/{len(FIXTURES)} correct\n")

hits = []
scanned = 0
for path in sorted(pathlib.Path('crates').rglob('*.rs')):
    src = path.read_text(encoding='utf-8', errors='replace')
    for name, body in test_bodies(src):
        scanned += 1
        if assertions_only_inside_a_vacuous_construct(body):
            hits.append((str(path), name))
# Verdicts from the 2026-09-02 sweep, so a later run re-litigates only what is
# new. Deliberately a list of NAMES rather than a suppression: a triaged test
# still prints, with what the ablation showed, because a silenced candidate is
# how a real one gets skipped after somebody rewrites it.
TRIAGED = {
    "every_extension_the_store_writes_can_be_read_back": "sound: iterates a literal written in the test",
    "a_declared_format_that_does_not_match_its_bytes_is_still_refused": "sound: iterates a literal",
    "apiary_handoff_commands_require_auth_and_hide_credentials": "sound: iterates a literal",
    "an_offer_the_fetcher_could_not_act_on_is_refused_before_it_reaches_one": "sound: iterates a literal",
    "current_schema_requires_hive_ownership_columns": "sound: iterates a literal",
    "an_interview_is_bounded_so_it_stays_an_instrument": "sound: iterates a literal",
    "a_degraded_subsystem_says_which_one_and_why": "sound: assert_eq! fixes the contents first",
    "task_and_worker_mutations_emit_typed_content_free_events": "sound: assert_eq! fixes the contents first",
    "filing_work_a_worker_cannot_route_tells_queen": "sound: an expect() earlier proves the query returns rows",
    "queen_filing_her_own_draft_tells_no_one": (
        "WAS REAL, fixed 2026-09-02. setup() left Queen with no session, so the rule "
        "`role != Queen && let Some(session_id)` short-circuited on the SESSION and never "
        "reached the role check. Deleting the Queen exemption entirely left it GREEN. "
        "Binding a session makes the same ablation fail, so the check was real and out of reach."
    ),
}

print(f"tests scanned                                   : {scanned}")
print(f"assertions ONLY inside an empty-safe construct   : {len(hits)}")
untriaged = 0
for f, n in hits:
    verdict = TRIAGED.get(n)
    if verdict:
        print(f"    {f}::{n}\n        triaged: {verdict}")
    else:
        untriaged += 1
        print(f"    {f}::{n}\n        NOT YET ABLATED -- a candidate, not a finding")
print(f"\nuntriaged candidates needing ablation           : {untriaged}")
