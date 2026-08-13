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

    const surfaceResults = [];
    for (const target of [
      { name: "needs-you", nav: /Needs you/, ready: () => page.getByRole("heading", { name: "Needs you" }) },
      { name: "tasks", nav: /Tasks/, ready: () => page.getByRole("heading", { name: "Task board" }) },
      { name: "workers", nav: /Workers/, ready: () => page.locator(".terminal-panel") },
      { name: "settings", nav: /Settings/, ready: () => page.getByRole("heading", { name: "Settings" }) },
    ]) {
      await page.getByRole("button", { name: target.nav }).click();
      await target.ready().first().waitFor();
      await page.waitForTimeout(250);
      const dimensions = await page.evaluate(() => ({
        scrollWidth: document.documentElement.scrollWidth,
        clientWidth: document.documentElement.clientWidth,
      }));
      if (dimensions.scrollWidth > dimensions.clientWidth + 1) {
        throw new Error(`${surface.name}/${target.name}: horizontal overflow ${dimensions.scrollWidth}px > ${dimensions.clientWidth}px`);
      }
      await page.screenshot({
        path: path.join(outputRoot, `${surface.name}-${target.name}.png`),
        fullPage: true,
      });
      surfaceResults.push({ surface: target.name, ...dimensions });
    }

    await page.getByRole("heading", { name: "Your familiar crew" }).waitFor();
    const provider = page.getByLabel("Coding provider");
    const codexDisabled = await provider.locator('option[value="codex"]').isDisabled();
    await page.getByText(/^(Current|Update waiting) ·/).waitFor();
    const workerEngineText = await page.getByText("Worker engine", { exact: true }).locator("..").innerText();
    if (!/Current|Update waiting/.test(workerEngineText)) {
      throw new Error(`${surface.name}: worker-engine maintenance state is unclear`);
    }
    let maintenanceConfirmation = false;
    if (workerEngineText.includes("Update waiting")) {
      await page.getByRole("button", { name: "Prepare worker engine update" }).click();
      const confirmation = page.getByRole("group", { name: "Confirm worker engine update" });
      await confirmation.waitFor();
      const dimensions = await page.evaluate(() => ({
        scrollWidth: document.documentElement.scrollWidth,
        clientWidth: document.documentElement.clientWidth,
      }));
      if (dimensions.scrollWidth > dimensions.clientWidth + 1) {
        throw new Error(`${surface.name}/maintenance: horizontal overflow ${dimensions.scrollWidth}px > ${dimensions.clientWidth}px`);
      }
      await page.screenshot({
        path: path.join(outputRoot, `${surface.name}-settings-maintenance.png`),
        fullPage: true,
      });
      await page.getByRole("button", { name: "Not now" }).click();
      maintenanceConfirmation = true;
    }
    if (consoleErrors.length || pageErrors.length) {
      throw new Error(`${surface.name}: browser errors: ${[...consoleErrors, ...pageErrors].join(" | ")}`);
    }
    await page.getByRole("button", { name: "Report a problem" }).click();
    await page.getByRole("dialog", { name: "Capture what felt wrong" }).waitFor();
    await page.getByLabel("What did you expect?").fill("Acceptance preview only");
    const privateSaveVisible = await page.getByRole("button", { name: "Save to this Hive" }).isVisible();
    if (!privateSaveVisible) throw new Error(`${surface.name}: private feedback save is unavailable`);
    await page.getByRole("button", { name: "Close" }).click();
    const backup = surface.mobile ? undefined : await verifyBackupDownload(page);
    return { surface: surface.name, surfaces: surfaceResults, codexDisabled, workerEngineText, maintenanceConfirmation, privateSaveVisible, backup, status: "passed" };
  } finally {
    await context.close();
  }
}

async function verifyBackupDownload(page) {
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "Download Hive backup" }).click(),
  ]);
  const stream = await download.createReadStream();
  let size = 0;
  let header = Buffer.alloc(0);
  for await (const chunk of stream) {
    const bytes = Buffer.from(chunk);
    size += bytes.length;
    if (header.length < 16) header = Buffer.concat([header, bytes]).subarray(0, 16);
  }
  await download.delete();
  if (header.toString("binary") !== "SQLite format 3\u0000" || size < 4096) {
    throw new Error(`desktop: Hive backup is not a valid SQLite artifact (${size} bytes)`);
  }
  return { sqliteHeader: true, bytes: size };
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exitCode = 1;
});
