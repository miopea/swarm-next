#!/usr/bin/env node

const { execFileSync } = require("node:child_process");
const fs = require("node:fs/promises");
const path = require("node:path");
const { chromium } = require("playwright");

const { evaluateGrowth, isTransientGatewayError, processTotals } = require("./browser-soak-metrics.cjs");

const baseUrl = process.env.SWARM_BASE_URL || "http://127.0.0.1:8766";
const operatorToken = process.env.SWARM_OPERATOR_TOKEN;
const browserExecutable = process.env.SWARM_BROWSER_EXECUTABLE;
const durationSeconds = boundedInteger("SWARM_BROWSER_SOAK_DURATION_SECONDS", 1800, 60, 86400);
const sampleSeconds = boundedInteger("SWARM_BROWSER_SOAK_SAMPLE_SECONDS", 30, 5, 300);
const activitySeconds = boundedInteger("SWARM_BROWSER_SOAK_ACTIVITY_SECONDS", 60, 15, 600);
const outputRoot = process.env.SWARM_BROWSER_SOAK_EVIDENCE || path.resolve("dist", "browser-soak");
const maxGatewayErrors = 20;

if (!operatorToken) throw new Error("SWARM_OPERATOR_TOKEN is required");

async function main() {
  await fs.mkdir(outputRoot, { recursive: true });
  const runId = `${new Date().toISOString().replaceAll(/[-:.]/g, "").slice(0, 15)}Z-browser`;
  const samplesPath = path.join(outputRoot, `${runId}-samples.csv`);
  const summaryPath = path.join(outputRoot, `${runId}-summary.json`);
  const browserErrors = [];
  const browser = await chromium.launch({
    headless: true,
    ...(browserExecutable ? { executablePath: browserExecutable } : {}),
  });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  page.on("console", (message) => { if (message.type() === "error") browserErrors.push(message.text()); });
  page.on("pageerror", (error) => browserErrors.push(error.message));
  const browserCdp = await browser.newBrowserCDPSession();
  const pageCdp = await context.newCDPSession(page);
  const samples = [];
  let pinnedBrowserPid;
  let nextActivityAt = activitySeconds;
  let activityCycles = 0;
  let handledBrowserErrors = 0;
  const updateRecoveries = [];

  try {
    await openAuthenticatedSettings(page);
    // A fresh browser probes its trusted session before the explicit unlock.
    // That expected 401 is outside the authenticated soak surface.
    browserErrors.length = 0;
    await pageCdp.send("Performance.enable");
    await fs.writeFile(samplesPath, [
      "timestamp,elapsed_seconds,browser_pid,process_count,storage_process_count,browser_working_set_bytes,browser_private_bytes,storage_working_set_bytes,storage_private_bytes,total_working_set_bytes,total_private_bytes,js_heap_bytes,dom_nodes,storage_usage_bytes,page_errors",
      "",
    ].join("\n"), "utf8");

    const startedAt = Date.now();
    while (true) {
      const elapsedSeconds = Math.round((Date.now() - startedAt) / 1000);
      const processInfo = (await browserCdp.send("SystemInfo.getProcessInfo")).processInfo;
      const browserProcess = processInfo.find((process) => process.type === "browser");
      if (!browserProcess) throw new Error("Edge did not report its browser process");
      pinnedBrowserPid ??= browserProcess.id;
      if (browserProcess.id !== pinnedBrowserPid) throw new Error(`browser process changed from ${pinnedBrowserPid} to ${browserProcess.id}`);
      const memory = readOwnedProcessMemory(processInfo.map((process) => process.id));
      const memoryByPid = new Map(memory.map((process) => [process.id, process]));
      if (!memoryByPid.has(pinnedBrowserPid)) throw new Error(`browser process ${pinnedBrowserPid} disappeared`);
      const browserMemory = memoryByPid.get(pinnedBrowserPid);
      const storagePids = new Set(processInfo.filter((process) => process.type.includes("StorageService")).map((process) => process.id));
      const storageMemory = processTotals(memory.filter((process) => storagePids.has(process.id)));
      const totals = processTotals(memory);
      const performanceMetrics = await pageCdp.send("Performance.getMetrics");
      const metric = new Map(performanceMetrics.metrics.map((entry) => [entry.name, entry.value]));
      const pageState = await page.evaluate(async () => ({
        storage_usage_bytes: (await navigator.storage.estimate()).usage ?? 0,
        dom_nodes: document.getElementsByTagName("*").length,
      }));
      const sample = {
        timestamp: new Date().toISOString(),
        elapsed_seconds: elapsedSeconds,
        browser_pid: pinnedBrowserPid,
        process_count: processInfo.length,
        storage_process_count: storagePids.size,
        browser_working_set_bytes: browserMemory.working_set_bytes,
        browser_private_bytes: browserMemory.private_bytes,
        storage_working_set_bytes: storageMemory.working_set_bytes,
        storage_private_bytes: storageMemory.private_bytes,
        total_working_set_bytes: totals.working_set_bytes,
        total_private_bytes: totals.private_bytes,
        js_heap_bytes: metric.get("JSHeapUsedSize") ?? 0,
        dom_nodes: pageState.dom_nodes,
        storage_usage_bytes: pageState.storage_usage_bytes,
        page_errors: browserErrors.length,
      };
      samples.push(sample);
      await fs.appendFile(samplesPath, `${Object.values(sample).join(",")}\n`, "utf8");
      if (browserErrors.length > handledBrowserErrors) {
        const newErrors = browserErrors.slice(handledBrowserErrors);
        if (browserErrors.length > maxGatewayErrors || newErrors.some((message) => !isTransientGatewayError(message))) {
          throw new Error(`authenticated page errors: ${newErrors.join(" | ")}`);
        }
        const recovery = await recoverAfterGatewayInterruption(page, elapsedSeconds);
        const recoveryErrors = browserErrors.slice(handledBrowserErrors);
        if (browserErrors.length > maxGatewayErrors || recoveryErrors.some((message) => !isTransientGatewayError(message))) {
          throw new Error(`authenticated page errors during recovery: ${recoveryErrors.join(" | ")}`);
        }
        recovery.error_count = recoveryErrors.length;
        updateRecoveries.push(recovery);
        handledBrowserErrors = browserErrors.length;
      }
      if (elapsedSeconds >= durationSeconds) break;
      if (elapsedSeconds >= nextActivityAt) {
        await exerciseReadOnlySurface(page);
        activityCycles += 1;
        nextActivityAt += activitySeconds;
      }
      await delay(Math.min(sampleSeconds, durationSeconds - elapsedSeconds) * 1000);
    }

    const browserPrivate = evaluateGrowth(samples, "browser_private_bytes");
    const storagePrivate = evaluateGrowth(samples, "storage_private_bytes", { growthLimit: 64 * 1024 * 1024, slopeLimit: 2 * 1024 * 1024 });
    const totalPrivate = evaluateGrowth(samples, "total_private_bytes", { growthLimit: 256 * 1024 * 1024, slopeLimit: 8 * 1024 * 1024 });
    const jsHeap = evaluateGrowth(samples, "js_heap_bytes", { growthLimit: 64 * 1024 * 1024, slopeLimit: 2 * 1024 * 1024 });
    const result = browserPrivate.passed && storagePrivate.passed && totalPrivate.passed && jsHeap.passed ? "passed" : "failed";
    const summary = {
      run_id: runId,
      result,
      base_url: baseUrl,
      duration_seconds: durationSeconds,
      sample_seconds: sampleSeconds,
      activity_seconds: activitySeconds,
      activity_cycles: activityCycles,
      sample_count: samples.length,
      browser_pid: pinnedBrowserPid,
      browser_private: browserPrivate,
      storage_private: storagePrivate,
      total_private: totalPrivate,
      renderer_js_heap: jsHeap,
      storage_usage_bytes: { min: Math.min(...samples.map((sample) => sample.storage_usage_bytes)), max: Math.max(...samples.map((sample) => sample.storage_usage_bytes)) },
      dom_nodes: { min: Math.min(...samples.map((sample) => sample.dom_nodes)), max: Math.max(...samples.map((sample) => sample.dom_nodes)) },
      process_count: { min: Math.min(...samples.map((sample) => sample.process_count)), max: Math.max(...samples.map((sample) => sample.process_count)) },
      storage_process_count: { min: Math.min(...samples.map((sample) => sample.storage_process_count)), max: Math.max(...samples.map((sample) => sample.storage_process_count)) },
      update_recoveries: updateRecoveries,
      samples_file: samplesPath,
    };
    await fs.writeFile(summaryPath, `${JSON.stringify(summary, null, 2)}\n`, "utf8");
    process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);
    if (result !== "passed") process.exitCode = 1;
  } catch (error) {
    const lastSample = samples.at(-1);
    const failure = {
      run_id: runId,
      result: "failed",
      base_url: baseUrl,
      duration_seconds_observed: lastSample?.elapsed_seconds ?? 0,
      sample_count: samples.length,
      browser_pid: pinnedBrowserPid,
      page_error_count: browserErrors.length,
      last_sample: lastSample ? {
        browser_private_bytes: lastSample.browser_private_bytes,
        storage_private_bytes: lastSample.storage_private_bytes,
        total_private_bytes: lastSample.total_private_bytes,
        js_heap_bytes: lastSample.js_heap_bytes,
        dom_nodes: lastSample.dom_nodes,
        storage_usage_bytes: lastSample.storage_usage_bytes,
      } : undefined,
      samples_file: samplesPath,
    };
    await fs.writeFile(summaryPath, `${JSON.stringify(failure, null, 2)}\n`, "utf8");
    throw error;
  } finally {
    await context.close();
    await browser.close();
  }
}

async function recoverAfterGatewayInterruption(page, elapsedSeconds) {
  const startedAt = Date.now();
  const cookieBefore = await browserSessionMetadata(page.context());
  const deadline = startedAt + 15_000;
  let healthy = false;
  while (Date.now() < deadline) {
    try {
      const response = await page.request.get(`${baseUrl}/health`, { timeout: 3_000 });
      healthy = response.ok();
    } catch {
      healthy = false;
    }
    if (healthy) break;
    await delay(500);
  }
  if (!healthy) throw new Error("gateway interruption did not recover within 15 seconds");
  await page.reload({ waitUntil: "domcontentloaded", timeout: 15_000 });
  try {
    await page.getByRole("button", { name: "Download Hive backup" }).waitFor({ timeout: 20_000 });
  } catch {
    const cookieAfter = await browserSessionMetadata(page.context());
    const sessionStatus = await page.request.get(`${baseUrl}/api/v1/auth/session`).then((response) => response.status()).catch(() => 0);
    throw new Error(`browser authentication did not survive the gateway interruption: before=${JSON.stringify(cookieBefore)} after=${JSON.stringify(cookieAfter)} session_status=${sessionStatus}`);
  }
  const runtimeStatus = (await page.locator(".rail-footer").innerText()).trim();
  if (!runtimeStatus.startsWith("Runtime 0.1.0-")) throw new Error(`runtime health did not recover after the gateway interruption: ${runtimeStatus}`);
  return { elapsed_seconds: elapsedSeconds, recovery_milliseconds: Date.now() - startedAt, runtime_status: runtimeStatus };
}

async function browserSessionMetadata(context) {
  const cookie = (await context.cookies(baseUrl)).find((candidate) => candidate.name === "swarm_next_operator_session");
  return cookie ? {
    present: true,
    value_length: cookie.value.length,
    secure: cookie.secure,
    same_site: cookie.sameSite,
    expires_in_seconds: Math.round(cookie.expires - Date.now() / 1000),
  } : { present: false };
}

async function exerciseReadOnlySurface(page) {
  for (const surface of [/Workers/, /Tasks/, /Settings/]) {
    await page.getByRole("button", { name: surface }).click();
    if (surface.source === "Workers") {
      const panel = page.locator(".terminal-panel");
      await panel.waitFor();
      try {
        await panel.getByText("connected", { exact: true }).waitFor();
      } catch {
        const state = (await panel.locator(".connection-state").textContent().catch(() => null))?.trim() || "missing";
        const detail = (await panel.locator(".terminal-toolbar small").first().textContent().catch(() => null))?.trim() || "none";
        const sessionId = (await panel.locator(".terminal-toolbar strong").textContent().catch(() => null))?.trim() || "missing";
        throw new Error(`terminal did not reconnect during navigation: session=${sessionId} state=${state} detail=${detail}`);
      }
    }
    if (surface.source === "Tasks") await page.getByRole("heading", { name: "Task board" }).waitFor();
    if (surface.source === "Settings") await page.getByRole("heading", { name: "Settings" }).waitFor();
  }
}

async function openAuthenticatedSettings(page) {
  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
  const tokenInput = page.getByLabel("Operator token");
  if (await tokenInput.isVisible().catch(() => false)) {
    await tokenInput.fill(operatorToken);
    await page.getByRole("button", { name: "Unlock Swarm" }).click();
  }
  await page.getByRole("button", { name: /Settings/ }).click();
  await page.getByRole("heading", { name: "Settings" }).waitFor();
  if (await tokenInput.isVisible().catch(() => false)) throw new Error("browser authentication did not complete");
}

function readOwnedProcessMemory(pids) {
  const uniquePids = [...new Set(pids.filter((pid) => Number.isSafeInteger(pid) && pid > 0))];
  if (process.platform === "win32") {
    const command = `$ids=@(${uniquePids.join(",")}); Get-Process -Id $ids -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject]@{id=[int]$_.Id;working_set_bytes=[long]$_.WorkingSet64;private_bytes=[long]$_.PrivateMemorySize64} } | ConvertTo-Json -Compress`;
    const output = execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], { encoding: "utf8", windowsHide: true }).trim();
    if (!output) return [];
    const parsed = JSON.parse(output);
    return Array.isArray(parsed) ? parsed : [parsed];
  }
  if (process.platform === "linux") {
    return uniquePids.flatMap((pid) => {
      try {
        const status = require("node:fs").readFileSync(`/proc/${pid}/status`, "utf8");
        const rss = Number(status.match(/^VmRSS:\s+(\d+)/m)?.[1] ?? 0) * 1024;
        const anon = Number(status.match(/^RssAnon:\s+(\d+)/m)?.[1] ?? 0) * 1024;
        return [{ id: pid, working_set_bytes: rss, private_bytes: anon || rss }];
      } catch {
        return [];
      }
    });
  }
  throw new Error(`process memory sampling is unsupported on ${process.platform}`);
}

function boundedInteger(name, fallback, minimum, maximum) {
  const value = Number(process.env[name] ?? fallback);
  if (!Number.isInteger(value) || value < minimum || value > maximum) {
    throw new Error(`${name} must be an integer from ${minimum} through ${maximum}`);
  }
  return value;
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

main().catch((error) => {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
});
