#!/usr/bin/env node

/**
 * The mobile journeys, driven in a REAL browser engine against fixtures only.
 *
 * ⚠️ WHAT THIS IS AND IS NOT. Operator ruling 01a0ad91-a1ad-7b73-8a99-70f53f8c034b,
 * verified at source, in their own words: "Use playwright, you don't need
 * browserstack". So this is Playwright, and it closes the ENGINE gap — a real
 * Chromium with a real mobile device profile, real sessionStorage, real event
 * ordering, a real <input type=file> — none of which jsdom provides. It does NOT
 * close the HARDWARE gap. It cannot say whether a physical iPhone's picker
 * offers a camera, or whether a real OS evicts a backgrounded tab. Anyone
 * reading a green run here should not record it as physical-device evidence.
 *
 * ⚠️ AND IT POINTS AT THE HARNESS, NEVER A HIVE. The other dogfood smokes drive
 * a real Hive with the operator token, which is safe because they run on this
 * machine. This one needs no token at all: vite.harness.config.ts serves a build
 * that cannot reach a Hive — no API proxy, production entry refused, fetch
 * stubbed — so everything below is fixture data by construction.
 *
 * WebKit would be the closer engine to iOS Safari and is deliberately not used:
 * `playwright install-deps webkit` needs root on this host, which is outside
 * what a repo worker may do. Chromium with a mobile profile is what is reachable
 * without asking for a password, and the gap is recorded rather than papered over.
 *
 * Start the harness first:  cd web && npm run harness
 */

const { chromium, devices } = require("playwright");

const harnessUrl = process.env.SWARM_HARNESS_URL || "http://127.0.0.1:5199";
const surface = `${harnessUrl}/harness.html?surface=terminal-composer`;
const DRAFT_KEY = "swarm.terminal-draft.v1";
const failures = [];

function check(name, condition, detail) {
  if (condition) return;
  failures.push(`${name}${detail ? `: ${detail}` : ""}`);
}

async function main() {
  const browser = await chromium.launch({ headless: true });
  // A real mobile device profile: coarse pointer, touch, mobile viewport.
  const context = await browser.newContext({ ...devices["Pixel 7"] });
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));

  try {
    await page.goto(surface, { waitUntil: "domcontentloaded" });

    // TIER 2, in a real engine rather than against a mocked matchMedia. The
    // branch this feeds decides whether opening a terminal also takes keyboard
    // focus; on a phone that would raise the keyboard over the view.
    check(
      "a mobile profile reports a coarse pointer",
      await page.evaluate(() => window.matchMedia("(pointer: coarse)").matches),
      "matchMedia said the emulated phone has a fine pointer",
    );

    // THE EVICTION JOURNEY, with real sessionStorage and real event ordering.
    const draft = page.getByLabel(/Message worker/);
    await draft.fill("half-written on a phone");

    check(
      "typing alone does not write to storage",
      (await page.evaluate((key) => window.sessionStorage.getItem(key), DRAFT_KEY)) === null,
      "a draft was persisted per keystroke, which this deliberately does not do",
    );

    // What the OS does to a backgrounded tab, driven through the real event.
    await page.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
      document.dispatchEvent(new Event("visibilitychange"));
    });

    const stored = await page.evaluate((key) => window.sessionStorage.getItem(key), DRAFT_KEY);
    check("backgrounding writes the draft", stored !== null && stored.includes("half-written on a phone"), String(stored));

    // The operator comes back to a tab the OS restored: a real reload, not a
    // re-render, so the draft has to come back out of real storage.
    await page.reload({ waitUntil: "domcontentloaded" });
    check(
      "the draft is recovered after a real reload",
      (await page.getByLabel(/Message worker/).inputValue()) === "half-written on a phone",
      await page.getByLabel(/Message worker/).inputValue(),
    );

    // WHAT A CAMERA HANDS OVER, through a real file input rather than a
    // constructed File. HEIC is what an iPhone actually produces.
    await page.setInputFiles("input[type=file]", {
      name: "IMG_0001.HEIC",
      mimeType: "image/heic",
      buffer: Buffer.from([0, 1, 2, 3]),
    });
    const picked = page.getByLabel("Fictional picked attachment");
    await picked.waitFor({ state: "attached" });
    const pickedText = (await picked.textContent()) ?? "";
    check("a HEIC photo reaches the composer with its own type", pickedText.includes("image/heic"), pickedText);

    check("no page errors", pageErrors.length === 0, pageErrors.join(" | "));
  } finally {
    await browser.close();
  }

  if (failures.length) {
    console.error(`mobile journeys: ${failures.length} FAILED`);
    for (const failure of failures) console.error(`  - ${failure}`);
    process.exitCode = 1;
    return;
  }
  console.log("mobile journeys: all checks passed (engine-level, NOT physical-device evidence)");
}

main().catch((error) => {
  console.error(`mobile journeys: could not run — ${error.message.split("\n")[0]}`);
  process.exitCode = 1;
});
