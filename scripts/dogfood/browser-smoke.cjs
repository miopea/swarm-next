#!/usr/bin/env node

const fs = require("node:fs/promises");
const path = require("node:path");
const { chromium } = require("playwright");

const baseUrl = process.env.SWARM_BASE_URL || "http://127.0.0.1:8766";
const operatorToken = process.env.SWARM_OPERATOR_TOKEN;
const browserExecutable = process.env.SWARM_BROWSER_EXECUTABLE;
const outputRoot = process.env.SWARM_BROWSER_EVIDENCE || path.resolve("dist", "browser-smoke");

if (!operatorToken) {
  throw new Error("SWARM_OPERATOR_TOKEN is required");
}

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
    for (const surface of surfaces) {
      results.push(await checkSurface(browser, surface));
    }
  } finally {
    await browser.close();
  }
  process.stdout.write(`${JSON.stringify({ baseUrl, results }, null, 2)}\n`);
}

async function checkSurface(browser, surface) {
  const context = await browser.newContext({
    viewport: surface.viewport,
    isMobile: surface.mobile,
    hasTouch: surface.mobile,
    ...(surface.mobile ? { userAgent: "Mozilla/5.0 (Linux; Android 15; Swarm Dogfood) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36" } : {}),
  });
  const page = await context.newPage();
  const consoleErrors = [];
  const pageErrors = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => pageErrors.push(error.message));

  try {
    await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
    const tokenInput = page.getByLabel("Operator token");
    if (await tokenInput.isVisible().catch(() => false)) {
      await tokenInput.fill(operatorToken);
      await page.getByRole("button", { name: "Unlock Swarm" }).click();
    }
    await page.getByRole("button", { name: /Workers/ }).waitFor();
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.getByRole("button", { name: /Workers/ }).waitFor();
    if (await tokenInput.isVisible().catch(() => false)) {
      throw new Error(`${surface.name}: browser session did not survive reload`);
    }
    // A fresh context probes the saved session before the first explicit unlock;
    // discard that expected 401 and inspect only the authenticated app below.
    consoleErrors.length = 0;
    pageErrors.length = 0;

    await page.getByRole("button", { name: /Settings/ }).click();
    await page.getByRole("heading", { name: "Your familiar crew" }).waitFor();
    const provider = page.getByLabel("Coding provider");
    const codexDisabled = await provider.locator('option[value="codex"]').isDisabled();
    await page.getByText(/^(Current|Update waiting) ·/).waitFor();
    const workerEngineText = await page.getByText("Worker engine", { exact: true }).locator("..").innerText();
    if (!/Current|Update waiting/.test(workerEngineText)) {
      throw new Error(`${surface.name}: worker-engine maintenance state is unclear`);
    }
    const dimensions = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    if (dimensions.scrollWidth > dimensions.clientWidth + 1) {
      throw new Error(`${surface.name}: horizontal overflow ${dimensions.scrollWidth}px > ${dimensions.clientWidth}px`);
    }
    if (consoleErrors.length || pageErrors.length) {
      throw new Error(`${surface.name}: browser errors: ${[...consoleErrors, ...pageErrors].join(" | ")}`);
    }
    await page.screenshot({
      path: path.join(outputRoot, `${surface.name}-settings.png`),
      fullPage: true,
    });
    return { surface: surface.name, ...dimensions, codexDisabled, workerEngineText, status: "passed" };
  } finally {
    await context.close();
  }
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exitCode = 1;
});
