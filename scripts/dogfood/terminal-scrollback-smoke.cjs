#!/usr/bin/env node

const fs = require("node:fs/promises");
const path = require("node:path");
const { chromium } = require("playwright");

const baseUrl = process.env.SWARM_BASE_URL || "http://127.0.0.1:8766";
const operatorToken = process.env.SWARM_OPERATOR_TOKEN;
const browserExecutable = process.env.SWARM_BROWSER_EXECUTABLE;
const outputRoot = process.env.SWARM_BROWSER_EVIDENCE || path.resolve("dist", "terminal-scrollback-smoke");

if (!operatorToken) throw new Error("SWARM_OPERATOR_TOKEN is required");

const surfaces = [
  { name: "desktop", viewport: { width: 1440, height: 900 }, mobile: false },
  { name: "mobile", viewport: { width: 412, height: 915 }, mobile: true },
];

async function main() {
  await fs.mkdir(outputRoot, { recursive: true });
  const browser = await chromium.launch({
    headless: true,
    ...(browserExecutable ? { executablePath: browserExecutable } : {}),
  });
  const results = [];
  try {
    for (const surface of surfaces) results.push(await verifySurface(browser, surface));
  } finally {
    await browser.close();
  }
  process.stdout.write(`${JSON.stringify({ baseUrl, results }, null, 2)}\n`);
}

async function verifySurface(browser, surface) {
  const context = await browser.newContext({
    viewport: surface.viewport,
    isMobile: surface.mobile,
    hasTouch: surface.mobile,
    ...(surface.mobile ? { userAgent: "Mozilla/5.0 (Linux; Android 15; Swarm Dogfood) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36" } : {}),
  });
  const page = await context.newPage();
  const errors = [];
  page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
    const tokenInput = page.getByLabel("Operator token");
    if (await tokenInput.isVisible().catch(() => false)) {
      await tokenInput.fill(operatorToken);
      await page.getByRole("button", { name: "Unlock Swarm" }).click();
    }
    await openQueen(page, surface.mobile);
    const before = await scrollbackMetrics(page);
    if (!before.scrollable) throw new Error(`${surface.name}: the active Queen has no browser scrollback to prove`);

    await page.reload({ waitUntil: "domcontentloaded" });
    await openQueen(page, surface.mobile);
    const after = await scrollbackMetrics(page);
    if (!after.scrollable) throw new Error(`${surface.name}: terminal scrollback disappeared after reload`);
    if (after.scrollHeight < before.clientHeight * 2) {
      throw new Error(`${surface.name}: restored terminal history is unexpectedly shallow (${JSON.stringify({ before, after })})`);
    }
    await page.locator(".xterm").hover();
    await page.mouse.wheel(0, -100_000);
    await page.screenshot({ path: path.join(outputRoot, `${surface.name}-restored-scrollback.png`), fullPage: false });
    if (errors.length) throw new Error(`${surface.name}: browser errors: ${errors.join(" | ")}`);
    return { surface: surface.name, before, after, status: "passed" };
  } finally {
    await context.close();
  }
}

async function openQueen(page, mobile) {
  const workersNavigation = page.getByRole("button", { name: /Workers/ });
  await workersNavigation.waitFor();
  await workersNavigation.click();
  if (mobile) {
    const switcher = page.locator(".mobile-worker-switcher-trigger");
    await switcher.waitFor();
    if (!/Queen/i.test(await switcher.innerText())) {
      await switcher.click();
      await page.locator(".mobile-worker-choice").filter({ hasText: "Queen" }).first().click();
    }
  } else {
    await page.locator(".worker-row .worker-button").filter({ hasText: "Queen" }).first().click();
  }
  await page.locator(".connection-connected").waitFor({ timeout: 15_000 });
  await page.locator(".xterm-viewport").waitFor();
  await page.waitForTimeout(500);
}

async function scrollbackMetrics(page) {
  return page.locator(".terminal-mount").evaluate((surface) => {
    const scrollbackRows = Number(surface.dataset.terminalScrollbackRows || 0);
    return {
      bufferLines: Number(surface.dataset.terminalBufferLines || 0),
      scrollbackRows,
      scrollable: scrollbackRows > 0,
    };
  });
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
