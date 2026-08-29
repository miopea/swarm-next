import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

/**
 * The harness runs on a config that CANNOT reach a Hive.
 *
 * The harness exists because a terminal is an xterm canvas that cannot be
 * redacted after the fact, and because real sessions on this box have carried
 * the operator's banking institutions and another product's password-policy
 * internals. Every procedure for publishing an image rests on one instruction:
 * capture from the harness, never from a Hive.
 *
 * Until this file existed, `npm run harness` started plain vite with the NORMAL
 * config — so port 5199 served `harness.html` (fixtures) and `index.html` (the
 * production app, proxied to the developer's API on 8765) side by side. One
 * path segment apart, both rendering a control room that looks broadly right,
 * and the wrong one full of real data that passes every downstream safety check
 * precisely because it is real. A worker was caught by it while capturing the
 * README screenshot the harness was built to make safe.
 *
 * Two mechanisms, because they close different holes:
 *
 *   NO PROXY. The normal config forwards /api (including WebSocket upgrades,
 *   which is how a terminal attaches) and /health to 127.0.0.1:8765. None of
 *   that is declared here, so nothing served on this port can reach a Hive even
 *   if the production app somehow renders. This is the real control.
 *
 *   ONE ENTRY. `harnessEntryOnly` refuses index.html and the production entry
 *   module, so the product cannot be rendered here at all. On its own this
 *   would be a signpost rather than a guard — someone could still request an
 *   API path directly — which is why it is the second mechanism and not the
 *   only one.
 *
 * `npm run dev` keeps using vite.config.ts and keeps proxying. That is what a
 * development server is for; only the harness must be incapable of it.
 */

/**
 * Serves the harness entry and refuses the product's.
 *
 * Registered directly in `configureServer`, which runs BEFORE vite's own
 * html-serving middleware — returning a function would run after it, by which
 * point index.html has already been sent.
 */
function harnessEntryOnly(): Plugin {
  const refused = new Set(["/", "/index.html", "/src/main.tsx"]);
  return {
    name: "swarm-harness-entry-only",
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        const path = (request.url ?? "/").split("?")[0];
        if (!refused.has(path)) {
          next();
          return;
        }
        response.statusCode = 404;
        response.setHeader("content-type", "text/plain; charset=utf-8");
        response.end(
          "The harness serves /harness.html only.\n\n" +
            "This port deliberately cannot render the product or reach a Hive: it has no\n" +
            "API proxy, and the production entry is refused. Anything captured here is\n" +
            "fixture data by construction.\n\n" +
            "For the real application against a real Hive, use `npm run dev`.\n",
        );
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), harnessEntryOnly()],
  // Without this, vite's SPA fallback serves index.html for EVERY unmatched
  // path — so blocking "/" and "/index.html" left the production app reachable
  // at /health, /anything, or a typo. Measured: /health returned the product's
  // HTML through the fallback while the block reported itself working. "mpa"
  // serves real html files only, so an unmatched path is a 404.
  appType: "mpa",
  server: {
    host: "127.0.0.1",
    port: 5199,
    strictPort: true,
    // No `proxy` key, deliberately. See the note above: this is the mechanism,
    // and adding one back here reopens the hazard silently.
  },
});
