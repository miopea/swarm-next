# Looking at the interface

Three UI changes shipped on 2026-08-28 and each handed back the same acceptance
line unmet:

| Change | What nobody had seen |
| --- | --- |
| `5d629b0` shared worker avatar | a real roster with 23 bees on it |
| `d42cd97` mobile wake/sleep | whether a 64px control is thumb-reachable |
| `cb1ffd5` Needs you layout | "checked at a narrow width" — declared undone outright |

Not for want of tooling. Seeing the running app needs a sign-in, and an agent
does not put the operator token into a login form. So the only eyes on the
interface were the operator's, and every UI task ended by telling them so.

## Run it

```bash
cd web && npm run harness      # vite on 127.0.0.1:5199, no API, no Hive
```

Then open `http://127.0.0.1:5199/harness.html` — a list of surfaces — or go
straight to one with `?surface=needs-you`.

Nothing here fetches, authenticates or touches a Hive. It renders the REAL
components against fixture props, so a screenshot of it is a screenshot of the
components that ship.

## What it is not

**It does not assert, and there are no baselines.** That is a decision, not an
omission. A screenshot diff fails on every legitimate change, and this repo had
three legitimate UI changes in one evening; a check that cries wolf gets
ignored, which is worse than no check.

**It is not in CI.** CI has no browser. It would either be skipped — a check
that cannot fail, which this codebase has spent a lot of effort deleting — or
add a browser to every run to compare images nobody looks at.

It is a thing a worker runs *before* claiming a UI change works.

## Measure, do not squint

The value is not only the picture. A browser can be asked for numbers, and a
number is what survives into a handoff:

```bash
rcg-browser exec <session> -- eval "() => { const d = document.querySelector('.unanswered-email-draft');
  const s = getComputedStyle(d);
  return { chars: Math.round(d.getBoundingClientRect().width / (parseFloat(s.fontSize) * 0.5)),
           scrolls: d.scrollHeight > d.clientHeight + 2 }; }"
```

## What it found the first time it was used

`cb1ffd5` had just "fixed" the letterboxed email draft and its tests passed.
Rendered:

- **Wide (1280):** the draft ran 919px at 12.16px — about **151 characters per
  line**. The cap was on height and there was none on width, so the fix made a
  long measure longer.
- **Narrow (390):** 439px visible against 480px of content. **It still scrolled,
  with 41px hidden** — the phone was doing exactly what the operator had
  complained about, on the task that existed to stop it.

Both were invisible to jsdom, which has no layout. `max-width: 68ch` and
`min(64vh, 520px)` were then chosen by measuring rather than guessing: 87
characters per line, and 480 of 480 on a phone with no scrollbar.

## Adding a surface

`web/src/harness/surfaces.tsx`. Compose the real components the way the app
composes them — the complaint that started this was cards competing with the
card above them, which no single-component render can show.

Transcribe fixtures from what the operator actually saw where you can. Inventing
plausible data renders a surface nobody has ever looked at.

## Publishing a picture of the interface

A screenshot of a running Hive is not a screenshot of the interface. It is a
screenshot of the operator's work, and the two are only the same when nothing
real is on the screen.

**The terminal cannot be anonymized.** It is an xterm canvas, so DOM mutation —
which is how everything else in a capture gets scrubbed — does not reach a single
character of it. Sessions inspected while producing the 0.9.2 marketing captures
held the operator's banking institutions and another product's password-policy
internals. **Never publish a capture of an actual worker session.** The only safe
way to show a terminal is composed demo material drawn over real UI chrome.

**Git makes a mistake permanent.** An image committed and then deleted is still
in the history, and a public repository has been cloned by then. That is why the
check happens before the commit rather than after someone notices.

So, before any capture is referenced from a committed file:

1. **Look at every image yourself.** Not the description of how it was made —
   the pixels. A scrub that silently failed and a scrub that worked produce
   identical prose and different pictures.
2. **Look for what DOM scrubbing cannot reach**: the terminal, anything else
   drawn to canvas, text baked into an image, and window or tab titles.
3. **Then look for what it can reach but may have missed**: client and project
   names, email addresses, identifiers, and paths under a home directory.

**Prefer the harness.** Everything above is a procedure for making a live
capture safe, and it is only ever as good as the person following it. The
harness has no Hive, no session and no real data behind it, so a capture taken
from it is safe by construction rather than by inspection. It cannot yet show
every screen — that is a reason to add surfaces to it, not a reason to go back
to photographing production.

**The published screenshots come from here**, and they mount the WHOLE APP:
`?surface=app&screen=decisions|tasks|workers`. That surface renders the real
`App` while `harness/hiveFixture.tsx` answers the network, so the picture has a
rail, a header and navigation because it IS the application, not a component
lifted out of it.

That distinction is the whole reason this section exists. The first attempt
captured single surfaces — `?surface=needs-you`, `?surface=tasks` — and each
picture was a panel floating on a beige field with no way to tell what
application it belonged to. They reached a public README before the operator
said "that makes no sense". A capture was checked for leaked data and for
whether the component rendered, and never for whether a stranger could tell
what it was of. **Ask that third question.**

Extracting the shell out of `App.tsx` was the obvious alternative and was
measured before being rejected: 236 lines of JSX, thirty call sites, thirty-four
interpolated identifiers — a sixty-prop refactor of the navigation, risked on a
live control room, for three pictures. Stubbing the network instead costs
nothing in production and yields a truer image.

**The terminal can be photographed, and only from here.** A worker view is
mostly an xterm CANVAS, so it is the one screen where the anonymise-then-capture
procedure above does not work at all: there is no node to rewrite and no regex
that reaches pixels. The sessions inspected for the 0.9.2 captures held the
operator's banking institutions and another product's password-policy internals.
So the rule has no exception — **never publish a capture of a real worker
session** — and the harness is what makes the picture possible instead.

Three pieces make it work, all of them at the harness boundary, none of them in
production code:

- `harness/terminalFixture.ts` invents the session. It replaces `WebSocket`
  the same way `main.tsx` already replaces `fetch`, and answers the terminal's
  `resume` with a canonical snapshot frame carrying invented output. Threading a
  `websocketFactory` prop down instead was rejected for the reason the fetch stub
  was: the seam has to be the network, or every socket added later escapes it.
- `main.tsx` denies WebGL2. `XtermSurface` falls back to the DOM renderer when
  WebGL is unavailable — a path that exists for headless environments — and that
  fallback is the difference between a screenshot and a blank rectangle. **A
  WebGL terminal photographs empty**: xterm reported all 35 lines in its buffer,
  `aria-busy` was down and the canvas was visible at full opacity, and the
  capture still came back an unbroken dark block. A picture that looks like a
  dead terminal while the terminal is fine is exactly the sort of evidence that
  gets believed.
- `harness.html` hides two xterm internals the DOM renderer exposes and WebGL
  never draws: the helper textarea, which photographs as a white box over the
  first row, and the char-measure span, which is `position: static` and pushed
  every line down by a row until it was taken out of flow.

**Match the transcript to the roster.** The first version of this capture put
Orchard Web's session under a header that said Queen, and the second put a
promo-code bug under a task about a redirect loop. Both rendered perfectly and
both were wrong in the only way that matters for a README: a reader who looks at
the picture for five seconds sees a product that contradicts itself. The
transcript, the selected worker and that worker's task in `productFixtures.tsx`
all have to tell one story.

    cd web && npm run harness
    # then, at 1440x900, select Orchard Web in the roster:
    http://127.0.0.1:5199/harness.html?surface=app&screen=workers

**Two Needs-you surfaces exist, and the difference matters.** `needs-you` is
transcribed from the operator's real screen, which is what makes it useful for
debugging and unpublishable: it carries their name, a real reply, real project
names and a real credential name. `needs-you-demo` is the one that goes in the
README. Capturing the wrong one is a single character in a URL, so check the
pixels before committing, every time.

**A screenshot dates faster than prose.** The 0.9.2 captures show the briefing
list before `d133349` gave it a rule and an indent, which is to say they show the
exact layout an operator had just called unbalanced. Prose describing a screen
survives a redesign; a picture of it does not. Re-take them when the surface they
show changes, and treat a stale screenshot as a defect rather than as old news.
