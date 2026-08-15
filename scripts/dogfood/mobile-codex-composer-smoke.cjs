#!/usr/bin/env node

const fs = require("node:fs/promises");
const path = require("node:path");
const { chromium } = require("playwright");

const baseUrl = process.env.SWARM_BASE_URL || "http://127.0.0.1:8766";
const operatorToken = process.env.SWARM_OPERATOR_TOKEN;
const browserExecutable = process.env.SWARM_BROWSER_EXECUTABLE;
const workerName = process.env.SWARM_CODEX_WORKER || "Codex Clover";
const outputRoot = process.env.SWARM_BROWSER_EVIDENCE || path.resolve("dist", "mobile-codex-composer-smoke");
const expectedMarker = "MOBILE_CODEX_SUBMIT_GAMMA";

if (!operatorToken) throw new Error("SWARM_OPERATOR_TOKEN is required");

async function main() {
  await fs.mkdir(outputRoot, { recursive: true });
  const browser = await chromium.launch({
    headless: true,
    ...(browserExecutable ? { executablePath: browserExecutable } : {}),
  });
  const context = await browser.newContext({
    viewport: { width: 412, height: 915 },
    isMobile: true,
    hasTouch: true,
    userAgent: "Mozilla/5.0 (Linux; Android 15; Swarm Dogfood) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36",
  });
  const page = await context.newPage();
  const errors = [];
  let wokeWorker = false;
  page.on("console", (message) => {
    if (message.type() === "error" && !message.text().includes("static.cloudflareinsights.com/beacon.min.js")) errors.push(message.text());
  });
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
    const tokenInput = page.getByLabel("Operator token");
    if (await tokenInput.isVisible().catch(() => false)) {
      await tokenInput.fill(operatorToken);
      await page.getByRole("button", { name: "Unlock Swarm" }).click();
    }
    // A locked first navigation can reject private bootstrap reads before the
    // explicit unlock succeeds. Judge only the authenticated operator journey.
    errors.length = 0;
    await page.getByRole("button", { name: /Workers/ }).click();
    const switcher = page.locator(".mobile-worker-switcher-trigger");
    await switcher.waitFor();
    if (!new RegExp(workerName, "i").test(await switcher.innerText())) {
      await switcher.click();
      const choice = page.locator(".mobile-worker-choice").filter({ hasText: workerName }).first();
      wokeWorker = /Sleeping/i.test(await choice.innerText());
      await choice.click();
    }
    await page.locator(".connection-connected").waitFor({ timeout: 30_000 });
    await page.locator(".xterm-rows").waitFor();
    // A resumed terminal may restore directly at the prompt without retaining
    // the startup banner in its visible rows. Connection plus a bounded settle
    // interval is the stable readiness boundary available to an operator.
    await page.waitForTimeout(6_000);
    const composer = page.getByLabel("Message worker");
    await composer.fill("Output the uppercase version of mobile_codex_submit_gamma only. Do not modify files.");
    await page.getByRole("button", { name: "Send" }).click();
    try {
      await page.waitForFunction(
        (marker) => document.querySelector(".xterm-rows")?.textContent?.includes(marker),
        expectedMarker,
        { timeout: 45_000 },
      );
    } catch {
      const visibleTerminal = await page.locator(".xterm-rows").textContent().catch(() => "");
      await page.screenshot({ path: path.join(outputRoot, "android-codex-submit-failed.png"), fullPage: false });
      throw new Error(`Codex mobile submission marker was not visible; terminal tail: ${visibleTerminal?.slice(-800)}`);
    }
    if (errors.length) throw new Error(`browser errors: ${errors.join(" | ")}`);
    await page.screenshot({ path: path.join(outputRoot, "android-codex-submitted.png"), fullPage: false });
    process.stdout.write(`${JSON.stringify({ baseUrl, workerName, expectedMarker, wokeWorker, status: "passed" }, null, 2)}\n`);
  } finally {
    if (wokeWorker) {
      const sleep = page.getByRole("button", { name: "Put worker to sleep" });
      if (await sleep.isVisible().catch(() => false)) await sleep.click().catch(() => undefined);
    }
    await context.close();
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
