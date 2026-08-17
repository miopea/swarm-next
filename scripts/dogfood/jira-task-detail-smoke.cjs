#!/usr/bin/env node

const fs = require("node:fs/promises");
const path = require("node:path");
const { chromium } = require("playwright");

const baseUrl = process.env.SWARM_BASE_URL || "http://127.0.0.1:8766";
const operatorToken = process.env.SWARM_OPERATOR_TOKEN;
const browserExecutable = process.env.SWARM_BROWSER_EXECUTABLE;
const issueKey = process.env.SWARM_JIRA_DETAIL_ISSUE || "WWD-4976";
const evidenceRoot = process.env.SWARM_BROWSER_EVIDENCE;

if (!operatorToken) throw new Error("SWARM_OPERATOR_TOKEN is required");

const surfaces = [
  { name: "desktop", viewport: { width: 1440, height: 900 }, mobile: false },
  { name: "mobile", viewport: { width: 412, height: 915 }, mobile: true },
];

async function main() {
  if (evidenceRoot) await fs.mkdir(evidenceRoot, { recursive: true });
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
  process.stdout.write(`${JSON.stringify({ baseUrl, issueKey, surfaces: results }, null, 2)}\n`);
}

async function verifySurface(browser, surface) {
  const context = await browser.newContext({
    viewport: surface.viewport,
    isMobile: surface.mobile,
    hasTouch: surface.mobile,
    ...(surface.mobile
      ? { userAgent: "Mozilla/5.0 (Linux; Android 15; Swarm Dogfood) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36" }
      : {}),
  });
  const page = await context.newPage();
  const errors = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
    const tokenInput = page.getByLabel("Operator token");
    if (await tokenInput.isVisible().catch(() => false)) {
      await tokenInput.fill(operatorToken);
      await page.getByRole("button", { name: "Unlock Swarm" }).click();
    }
    await page.getByRole("button", { name: /Tasks/ }).click();
    // A fresh browser intentionally receives one 401 while probing for an
    // existing trusted session before the operator token is submitted.
    errors.length = 0;
    const task = page.locator("article").filter({ hasText: issueKey }).first();
    await task.waitFor({ state: "visible" });
    await task.dblclick();
    const dialog = page.getByRole("dialog");
    await dialog.waitFor({ state: "visible" });
    const image = dialog.getByRole("img").first();
    await image.waitFor({ state: "visible" });
    await page.waitForFunction(
      (key) => {
        const openDialog = document.querySelector('[role="dialog"]');
        const candidate = openDialog?.querySelector("img");
        return openDialog?.textContent?.includes(key) && candidate?.complete && candidate.naturalWidth > 0;
      },
      issueKey,
    );
    const metrics = await page.evaluate(() => {
      const root = document.documentElement;
      const openDialog = document.querySelector('[role="dialog"]');
      const imageElement = openDialog?.querySelector("img");
      const dialogRect = openDialog?.getBoundingClientRect();
      return {
        viewportWidth: window.innerWidth,
        documentClientWidth: root.clientWidth,
        documentScrollWidth: root.scrollWidth,
        dialog: dialogRect
          ? { x: dialogRect.x, width: dialogRect.width, right: dialogRect.right }
          : null,
        image: imageElement
          ? {
              complete: imageElement.complete,
              naturalWidth: imageElement.naturalWidth,
              naturalHeight: imageElement.naturalHeight,
            }
          : null,
      };
    });
    if (metrics.documentScrollWidth > metrics.documentClientWidth) {
      throw new Error(`${surface.name} task detail caused horizontal page overflow`);
    }
    if (!metrics.dialog || metrics.dialog.x < 0 || metrics.dialog.right > metrics.viewportWidth + 1) {
      throw new Error(`${surface.name} task detail escaped the viewport`);
    }
    if (!metrics.image?.complete || metrics.image.naturalWidth < 1 || metrics.image.naturalHeight < 1) {
      throw new Error(`${surface.name} Jira image did not decode`);
    }
    if (evidenceRoot) {
      await page.screenshot({
        path: path.join(evidenceRoot, `jira-task-detail-${surface.name}.png`),
        fullPage: false,
      });
    }
    if (errors.length) throw new Error(`${surface.name} browser errors: ${errors.join(" | ")}`);
    return { surface: surface.name, status: "ok", metrics };
  } finally {
    await context.close();
  }
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error}\n`);
  process.exitCode = 1;
});
