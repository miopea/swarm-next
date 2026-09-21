# ADR 0107: A watch is recorded; a frame is not

Status: Accepted from the operator interview of 2026-09-21, recorded in
`docs/specs/apiary-controls-scope.md`. This ADR covers the CONTROL PLANE only;
the frame relay it authorizes is not yet built.

Watching a member Hive is a full live window, the same depth for Keeper and for
a Steward in scope. The grant decides WHICH Hives, never HOW MUCH.

## The two properties are the reason this is allowed to exist

The operator asked for "literally everything, basically a window into that
hive". Without both of the following, that is standing surveillance of another
person's machine and should not ship:

1. **Live only, never stored.**
2. **Always visible while it lasts.**

Both had to become structure rather than intention, because both are the kind of
promise that a later change breaks silently.

## What is recorded, and what cannot be

`apiary_watches` and `local_federation_watches` hold WHO watched WHICH Hive and
WHEN. There is no column on either table that could hold terminal output. A
relay that wanted to persist a frame would have to add one — the absence is the
enforcement, not a convention, and it is the same move as `HiveCapabilityWorker`
having no path field.

So a compromised Keeper exposes what is on one screen right now, rather than
months of every member's terminal history including whatever they typed. The
member Hive keeps its own ordinary private bounded history, as it does today.

## Visibility is a sequence, not a promise

A watch is created `requested`. It becomes `active` — the only state that
relays — when the TARGET acknowledges. The member's reconcile writes its local
mirror BEFORE it acknowledges, so an acknowledgement means "this Hive has the
watch on its own screen". Acknowledging first would licence a relay against a
Hive whose operator can still see nothing.

The notice is read app-wide from the member's OWN mirror, so it keeps saying so
while Keeper is unreachable, and it is shown from `requested` onward — the
operator learns a window is opening rather than finding out once someone is
already looking. `WatchedByNotice` failing to render is therefore not a cosmetic
bug but the forbidden state, which is why it tolerates any body shape.

## One authority function, two doors

Keeper's own operator opens a watch locally; a Steward asks over its outbound
connection. Both reach `open_apiary_watch`, which takes no depth parameter
because there is no depth to vary. Two entry points that each did their own
checking is how two observation depths get built by accident.

Authority is rechecked at Keeper against the grants as they stand, so revoking a
stewardship stops it working immediately rather than when something cached
expires.

## A watch ends by itself

Five minutes, renewed only while the watcher's window is open, matching the ADR
0036 takeover lease rather than inventing a second number. Renewal refuses a
lapsed watch: reviving one would reopen the window with no notice beside it. The
WATCHED operator may end it from their own machine, for the same reason ADR 0036
grants instant reclaim — the person at the machine may be mid-incident and is
the only one who knows, and the watcher loses a window they can simply reopen.

## What this cannot answer, deliberately

There is no reason field. The operator was offered "who and why" and chose "who,
no reason required", to keep a quick glance low-friction. This is knowingly
asymmetric with ADR 0036, which requires and audits a reason for takeover. The
audit answers "who looked, and when" and can NEVER answer "why". Do not quietly
add a required reason later without saying that it reverses this.

## Consequence the operator should hear

`normalize_stewardship_grant` refuses any stewardship that does not include
Observe, so an assist-only Steward cannot be created. Upgrading Observe in place
therefore widens EVERY stewardship that will ever exist, not only ones whose
Keeper chose to tick it. Checked against the live store on 2026-09-21: one
Apiary, one stewardship, four capability grants over two Hives, eight
memberships. Whoever holds that stewardship gains terminal visibility without
having agreed to it, and should be told rather than discover it.

## Status of the relay

Not built. No frame crosses the Apiary yet, and no route exposes one. The
control plane shipping first is deliberate: it makes an invisible watch
unreachable before anything can be relayed, rather than leaving a window in
which watching works and the notice does not.
