import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "../styles.css";
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
 * IT IS NOT A TEST AND DOES NOT ASSERT. There are no baselines on purpose:
 * a screenshot diff fails on every legitimate change, and this repo had three
 * legitimate changes in one evening. A check that cries wolf gets ignored,
 * which is worse than no check. This is for looking before claiming.
 */
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
