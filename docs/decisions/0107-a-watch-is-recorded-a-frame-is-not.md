# ADR 0107: A watch is recorded; a frame is not

Status: Accepted from the operator interview of 2026-09-21, recorded in
`docs/specs/apiary-controls-scope.md`. The control plane, the frame relay and
the Keeper's viewer surface are built.

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

## The relay carries bytes and keeps none

Frames travel member → Keeper → watcher over two sockets, both outbound from the
member's side of the trust boundary. Keeper holds one bounded in-memory channel
per watch and forwards opaque bytes; it never parses a frame, so there is nothing
in the relay that could grow into a transcript. `watch_relay` has no `TaskStore`,
and giving it one would be reversing this ADR.

A viewer that falls behind is DROPPED rather than buffered. A doorbell can ring
once for everything it missed; a terminal cannot be resynchronised from a gap,
and a generous buffer would quietly become the recent-history store this design
forbids. The viewer reconnects to a fresh snapshot, which is the only honest
answer.

Frames use the SAME wire format as the local terminal socket, byte for byte, so
a watcher renders a remote Hive with the code that renders their own. A second
format would be a second thing to keep correct, and the two would drift the first
time either changed.

Both sockets RE-READ the lease every fifteen seconds rather than trusting the
authorization they opened under, and the producing side re-reads it every poll —
so a watch ended from the watched machine stops production there, without waiting
for Keeper to hang up.

## The viewer holds a ticket, not a credential

A browser CANNOT send an `Authorization` header on a WebSocket. The first viewer
route read one, which meant it could only ever have been reached from a test
that could set headers — it was unreachable from the UI it existed for. Putting
the operator token in a subprotocol instead would leak a long-lived credential
into a string proxies and logs routinely record.

So the viewer fetches a single-use, 30-second grant over an ordinary request and
offers THAT as the subprotocol. The socket re-checks the watch after spending
the grant: the grant proves who asked, and the lease proves the watch is still
theirs and still live. The server echoes the selected subprotocol, without which
the handshake fails — the same shape of unreachability as the header bug, and
worth stating because nothing else makes it obvious.

## Watching has no input path, structurally

The window has no keyboard handler and no send method. Typing into another
operator's machine is TAKEOVER, which ADR 0036 governs separately and gates
behind a reasoned, audited, exclusive lease. `WatchStream` is deliberately not
`TerminalConnection` for this reason: reusing that class would have brought
attach grants, engagement leases and a write path into a surface that must not
have one.

## What the window does not yet do

It relays QUEEN's terminal, which is narrower than "literally everything". ADR
0036 relays exactly this for takeover and it is the surface with a precedent;
widening to a chosen session needs the watcher to be able to ASK for one, which
this one-way push deliberately cannot carry.

A Steward watching from their own Hive needs one more hop — their Hive proxying
Keeper's viewer socket — which is not built. The AUTHORITY is already identical,
so this is transport rather than a second depth.
