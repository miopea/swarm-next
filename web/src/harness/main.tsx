import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "../styles.css";
import { hiveFixture } from "./hiveFixture";
import { FixtureWebSocket } from "./terminalFixture";
import { SURFACES } from "./surfaces";

/**
 * A place to LOOK at the interface, with no Hive and no credential.
 *
 * WHY THIS EXISTS. Three UI changes shipped on 2026-08-28 and each handed back
 * the same acceptance line unmet — nobody had seen a roster of 23 bees, nobody
 * knew whether a 64px control was thumb-reachable, and "checked at a narrow
 * width" was declared undone on a task that was entirely about how a page
 * reads. Not because the tooling was missing: seeing the running app needs a
 * sign-in, and an agent will not put the operator token into a login form.
 *
 * So this renders the REAL components against fixture props. Nothing here
 * fetches, authenticates or touches a Hive. Open one surface at a time with
 * ?surface=<id>, which is how a browser can be pointed at exactly one thing.
 *
 * NOTHING REACHES THE NETWORK. Components that read for themselves are the
 * norm, not the exception, and each one that fails a fetch paints its own
 * "could not be refreshed" banner over whatever is being looked at. Handing
 * every such component a loader prop was tried and is the wrong shape: it caps
 * the fetches that exist today and every one added later escapes, which is the
 * same trap the enumerated CSS measure fell into. The network is the boundary,
 * so the network is what gets stubbed — once, here, for everything.
 *
 * IT IS NOT A TEST AND DOES NOT ASSERT. There are no baselines on purpose:
 * a screenshot diff fails on every legitimate change, and this repo had three
 * legitimate changes in one evening. A check that cries wolf gets ignored,
 * which is worse than no check. This is for looking before claiming.
 */
/**
 * Every API call answers empty, and none of them reach a Hive.
 *
 * An empty list is the right default: it is what a component sees on a Hive
 * with nothing in it, so it renders the state the fixture asked for rather than
 * an error about a server that was never there. Anything a surface genuinely
 * needs comes from its fixture props, not from here.
 */
const originalFetch = globalThis.fetch;
const json = (body: unknown) =>
  new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });

globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  // /health is not under /api, and letting it fall through to the dev server
  // returned 500 — which the app reads as "Runtime unavailable" and refuses to
  // render anything useful behind.
  if (!url.startsWith("/api/") && !url.includes("/api/v1/") && !url.endsWith("/health")) {
    return originalFetch(input as RequestInfo, init);
  }
  // A handful of endpoints answer with fixtures rather than nothing, which is
  // what lets the WHOLE APP mount here instead of a single card. Everything
  // else keeps answering empty.
  const path = url.split("?")[0];
  const served = hiveFixture(path);
  return served === undefined ? json([]) : json(served);
}) as typeof fetch;

/**
 * A socket is network, so it is stubbed where the network is stubbed.
 *
 * Without this the Workers screen renders an empty black canvas: the terminal
 * asks for an attach grant, dials a WebSocket, and nothing answers. The
 * fixture socket plays an invented session instead — which is the ONLY way a
 * terminal can be photographed at all, because a canvas cannot be scrubbed
 * after the fact the way the DOM can.
 */
globalThis.WebSocket = FixtureWebSocket as unknown as typeof WebSocket;

/**
 * The terminal draws with WebGL when it can, and a WebGL canvas photographs
 * BLANK. Not empty — blank: xterm's buffer held all 35 lines and reported them,
 * aria-busy was down, the canvas was visible at full opacity, and the capture
 * still came back as an unbroken dark rectangle. A screenshot that looks like a
 * dead terminal while the terminal is fine is exactly the kind of evidence that
 * gets believed.
 *
 * XtermSurface already falls back to the DOM renderer when WebGL2 is missing —
 * that path exists for headless environments and GPU denylists. Denying WebGL2
 * here takes it deliberately, so the glyphs are real DOM text that any browser
 * on any machine renders identically. Slower, and nothing here is racing.
 */
const originalGetContext = HTMLCanvasElement.prototype.getContext;
HTMLCanvasElement.prototype.getContext = function getContext(
  this: HTMLCanvasElement,
  contextId: string,
  ...rest: unknown[]
) {
  if (contextId === "webgl2" || contextId === "webgl" || contextId === "experimental-webgl") return null;
  return (originalGetContext as (this: HTMLCanvasElement, id: string, ...args: unknown[]) => unknown).call(
    this,
    contextId,
    ...rest,
  );
} as typeof HTMLCanvasElement.prototype.getContext;

const requested = new URLSearchParams(window.location.search).get("surface");
const surface = SURFACES.find((entry) => entry.id === requested);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {surface ? (
      surface.render()
    ) : (
      <main style={{ padding: 24, fontFamily: "system-ui", lineHeight: 1.6 }}>
        <h1>Swarm harness</h1>
        <p>Real components, fixture data, no Hive and no sign-in.</p>
        <ul>
          {SURFACES.map((entry) => (
            <li key={entry.id}>
              <a href={`?surface=${entry.id}`}>{entry.title}</a> — {entry.why}
            </li>
          ))}
        </ul>
      </main>
    )}
  </StrictMode>,
);
