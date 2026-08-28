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
