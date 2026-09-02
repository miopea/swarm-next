import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";

/**
 * A LOOPING ANIMATION MAY ONLY MOVE WHAT THE COMPOSITOR CAN MOVE.
 *
 * `transform` and `opacity` are handled on the compositor, so an infinite
 * animation of them costs no paint. Everything else — box-shadow, width, top,
 * background-color, filter — repaints the element on EVERY FRAME, sixty times a
 * second, for as long as the tab is open.
 *
 * The worker roster animated `box-shadow` on the presence dot of every buzzing
 * worker. On the default view with thirteen live sessions that is a continuous
 * repaint storm, and the operator measured it: "typically running 15% CPU in
 * Edge, which no website ever does." A previous performance pass did not find
 * it, because nothing looks wrong in the markup and the rule is one line.
 *
 * This reads the stylesheet rather than trusting review, because the cost is
 * invisible at the point of writing and only shows up on someone's fan.
 */
const COMPOSITED = new Set(["transform", "opacity"]);

test("no looping animation repaints on every frame", () => {
  const css = readFileSync(join(__dirname, "styles.css"), "utf8");

  const looping = new Set(
    [...css.matchAll(/animation:[^;]*?([\w-]+)\s+[^;]*infinite/g)].map((m) => m[1]),
  );
  expect(looping.size).toBeGreaterThan(0); // the scan found the animations at all

  // Brace-counted rather than regex-matched: these keyframes are written inline,
  // so a lazy match to the next newline-brace swallows the rest of the sheet and
  // reports thousands of false offenders. Ask me how I know.
  function keyframeBody(name: string): string | undefined {
    const start = css.search(new RegExp(`@keyframes\\s+${name}\\s*\\{`));
    if (start === -1) return undefined;
    let depth = 0;
    for (let i = css.indexOf("{", start); i < css.length; i += 1) {
      if (css[i] === "{") depth += 1;
      else if (css[i] === "}") {
        depth -= 1;
        if (depth === 0) return css.slice(css.indexOf("{", start) + 1, i);
      }
    }
    return undefined;
  }

  const offenders: string[] = [];
  for (const name of looping) {
    const body = keyframeBody(name);
    if (!body) continue;
    for (const [, property] of body.matchAll(/([\w-]+)\s*:/g)) {
      if (!COMPOSITED.has(property)) offenders.push(`${name} animates ${property}`);
    }
  }

  expect(offenders).toEqual([]);
});
