#!/usr/bin/env node

const fs = require("node:fs/promises");
const path = require("node:path");
const { chromium } = require("playwright");

const baseUrl = process.env.SWARM_BASE_URL || "http://127.0.0.1:8766";
const operatorToken = process.env.SWARM_OPERATOR_TOKEN;
const browserExecutable = process.env.SWARM_BROWSER_EXECUTABLE;
const workerName = process.env.SWARM_SCROLLBACK_WORKER || "Dogfood Clover";
const restoreWorkerAfterRun = process.env.SWARM_SCROLLBACK_RESTORE_SLEEP === "1";
const outputRoot = process.env.SWARM_BROWSER_EVIDENCE || path.resolve("dist", "terminal-scrollback-smoke");

if (!operatorToken) throw new Error("SWARM_OPERATOR_TOKEN is required");

const surfaces = [
  { name: "mobile", viewport: { width: 412, height: 915 }, mobile: true },
  { name: "desktop", viewport: { width: 1440, height: 900 }, mobile: false },
];

async function main() {
  await fs.mkdir(outputRoot, { recursive: true });
  const browser = await chromium.launch({
    headless: true,
    ...(browserExecutable ? { executablePath: browserExecutable } : {}),
  });
  const results = [];
  let restoreSleepingWorker = false;
  try {
    for (const [index, surface] of surfaces.entries()) {
      const result = await verifySurface(browser, surface, index === surfaces.length - 1, restoreSleepingWorker);
      restoreSleepingWorker ||= result.wokeWorker;
      results.push(result);
    }
  } finally {
    await browser.close();
  }
  process.stdout.write(`${JSON.stringify({ baseUrl, results }, null, 2)}\n`);
}

async function verifySurface(browser, surface, finalSurface, restoreSleepingWorker) {
  const context = await browser.newContext({
    viewport: surface.viewport,
    isMobile: surface.mobile,
    hasTouch: surface.mobile,
    ...(surface.mobile ? { userAgent: "Mozilla/5.0 (Linux; Android 15; Swarm Dogfood) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36" } : {}),
  });
  const page = await context.newPage();
  const errors = [];
  let wokeWorker = false;
  let passed = false;
  page.on("console", (message) => {
    if (message.type() === "error" && !ignoredBrowserError(message.text())) errors.push(message.text());
  });
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
    const tokenInput = page.getByLabel("Operator token");
    if (await tokenInput.isVisible().catch(() => false)) {
      await tokenInput.fill(operatorToken);
      await page.getByRole("button", { name: "Unlock Swarm" }).click();
    }
    wokeWorker = await openWorker(page, surface.mobile);
    // A locked first navigation can legitimately reject private bootstrap reads
    // before the explicit unlock succeeds. Judge the authenticated journey only.
    errors.length = 0;
    const before = await ensureScrollback(page, surface.mobile);

    await page.reload({ waitUntil: "domcontentloaded" });
    await openWorker(page, surface.mobile);
    const after = await scrollbackMetrics(page);
    if (!after.scrollable) throw new Error(`${surface.name}: terminal scrollback disappeared after reload`);
    if (after.scrollbackRows < Math.min(before.scrollbackRows, 100)) {
      throw new Error(`${surface.name}: restored terminal history is unexpectedly shallow (${JSON.stringify({ before, after })})`);
    }
    const beforeGesture = await scrollbackMetrics(page);
    if (surface.mobile) {
      await dragTerminalWithTouch(page, "down");
    } else {
      await scrollTerminalWithWheel(page);
    }
    await page.waitForTimeout(250);
    const afterGesture = await scrollbackMetrics(page);
    const gestureMoved = surface.mobile
      ? afterGesture.viewportRow < beforeGesture.viewportRow
      : afterGesture.viewportScrollTop < beforeGesture.viewportScrollTop;
    if (!gestureMoved) {
      throw new Error(`${surface.name}: terminal viewport did not move into scrollback (${JSON.stringify({ beforeGesture, afterGesture })})`);
    }
    await page.screenshot({ path: path.join(outputRoot, `${surface.name}-restored-scrollback.png`), fullPage: false });
    if (errors.length) throw new Error(`${surface.name}: browser errors: ${errors.join(" | ")}`);
    if (finalSurface && (restoreWorkerAfterRun || restoreSleepingWorker || wokeWorker)) {
      await page.getByRole("button", { name: "Put worker to sleep" }).click();
    }
    passed = true;
    return { surface: surface.name, before, after, beforeGesture, afterGesture, status: "passed", wokeWorker };
  } finally {
    if (!passed && (restoreWorkerAfterRun || restoreSleepingWorker || wokeWorker)) {
      const sleep = page.getByRole("button", { name: "Put worker to sleep" });
      if (await sleep.isVisible().catch(() => false)) await sleep.click().catch(() => undefined);
    }
    await context.close();
  }
}

async function scrollTerminalWithWheel(page) {
  const screen = page.locator(".xterm-screen");
  const box = await screen.boundingBox();
  if (!box) throw new Error("desktop: terminal screen has no wheel target");
  const session = await page.context().newCDPSession(page);
  try {
    await session.send("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: box.x + box.width / 2,
      y: box.y + box.height / 2,
    });
    await session.send("Input.dispatchMouseEvent", {
      type: "mouseWheel",
      x: box.x + box.width / 2,
      y: box.y + box.height / 2,
      deltaX: 0,
      deltaY: -100_000,
    });
  } finally {
    await session.detach();
  }
}

async function dragTerminalWithTouch(page, direction) {
  const screen = page.locator(".xterm-screen");
  const box = await screen.boundingBox();
  if (!box) throw new Error("mobile: terminal screen has no touch target");
  const x = box.x + box.width / 2;
  const top = box.y + Math.min(80, box.height * 0.25);
  const bottom = box.y + Math.max(box.height - 80, box.height * 0.75);
  const startY = direction === "down" ? top : bottom;
  const endY = direction === "down" ? bottom : top;
  const session = await page.context().newCDPSession(page);
  try {
    await session.send("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [{ x, y: startY, id: 1 }],
    });
    for (let step = 1; step <= 6; step += 1) {
      await session.send("Input.dispatchTouchEvent", {
        type: "touchMove",
        touchPoints: [{ x, y: startY + ((endY - startY) * step) / 6, id: 1 }],
      });
    }
    await session.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  } finally {
    await session.detach();
  }
}

function ignoredBrowserError(message) {
  return message.includes("static.cloudflareinsights.com/beacon.min.js")
    && message.includes("Content Security Policy");
}

async function ensureScrollback(page, mobile) {
  let metrics = await scrollbackMetrics(page);
  if (metrics.scrollable) return metrics;

  // The holder may have restarted from a legacy visible-screen-only checkpoint.
  // Prime bounded history with a content-only validation turn so this smoke test
  // proves the new snapshot path instead of depending on prior operator activity.
  if (mobile) {
    const composer = page.getByLabel("Message worker");
    await composer.fill("Terminal scrollback validation only: output exactly 250 short numbered lines, then stop. Do not call tools or change files.");
    await page.getByRole("button", { name: "Send" }).click();
  } else {
    const terminalInput = page.locator(".xterm-helper-textarea");
    await terminalInput.type("Terminal scrollback validation only: output exactly 250 short numbered lines, then stop. Do not call tools or change files.");
    await terminalInput.press("Enter");
  }
  for (let attempt = 0; attempt < 120; attempt += 1) {
    await page.waitForTimeout(250);
    metrics = await scrollbackMetrics(page);
    if (metrics.scrollable) return metrics;
  }
  await page.screenshot({ path: path.join(outputRoot, `${mobile ? "mobile" : "desktop"}-scrollback-prime-failed.png`), fullPage: false });
  throw new Error(`${workerName} did not create browser scrollback after bounded validation output (${JSON.stringify(metrics)})`);
}

async function openWorker(page, mobile) {
  const workersNavigation = page.getByRole("button", { name: /Workers/ });
  await workersNavigation.waitFor();
  await workersNavigation.click();
  if (mobile) {
    const switcher = page.locator(".mobile-worker-switcher-trigger");
    await switcher.waitFor();
    if (!new RegExp(workerName, "i").test(await switcher.innerText())) {
      await switcher.click();
      const choice = page.locator(".mobile-worker-choice").filter({ hasText: workerName }).first();
      const sleeping = /Sleeping/i.test(await choice.innerText());
      await choice.click();
      await page.locator(".connection-connected").waitFor({ timeout: 30_000 });
      await page.locator(".xterm-viewport").waitFor();
      await page.waitForTimeout(500);
      return sleeping;
    }
  } else {
    const choice = page.locator(".worker-row .worker-button").filter({ hasText: workerName }).first();
    const sleeping = /Sleeping/i.test(await choice.innerText());
    await choice.click();
    await page.locator(".connection-connected").waitFor({ timeout: 30_000 });
    await page.locator(".xterm-viewport").waitFor();
    await page.waitForTimeout(500);
    return sleeping;
  }
  await page.locator(".connection-connected").waitFor({ timeout: 15_000 });
  await page.locator(".xterm-viewport").waitFor();
  await page.waitForTimeout(500);
  return false;
}

async function scrollbackMetrics(page) {
  return page.locator(".terminal-surface").evaluate((surface) => {
    const scrollbackRows = Number(surface.dataset.terminalScrollbackRows || 0);
    return {
      attributes: Object.fromEntries([...surface.attributes].map((attribute) => [attribute.name, attribute.value])),
      bufferLines: Number(surface.dataset.terminalBufferLines || 0),
      childCount: surface.childElementCount,
      scrollbackRows,
      viewportScrollTop: surface.querySelector(".xterm-viewport")?.scrollTop ?? 0,
      viewportRow: Number(surface.dataset.terminalViewportRow || 0),
      scrollable: scrollbackRows > 0,
    };
  });
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
