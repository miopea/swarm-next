#!/usr/bin/env node
// Drives two real Hives through what only exists between two Hives. Started by
// ../two-hive-acceptance.sh, which owns the processes. Prints one line per
// check and exits non-zero on the first failure, naming what it saw.
const { execFileSync } = require("node:child_process");

const fs = require("node:fs");

const { KEEPER, MEMBER, TOKEN, KEEPER_UNIT, KEEPER_START, PROXY_FAULT, ARTIFACTS } = process.env;
const started = Date.now();
const elapsed = () => `${((Date.now() - started) / 1000).toFixed(1)}s`;

async function api(base, method, path, body) {
  const response = await fetch(`${base}${path}`, {
    method,
    headers: { authorization: `Bearer ${TOKEN}`, ...(body === undefined ? {} : { "content-type": "application/json" }) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  let json;
  try { json = text ? JSON.parse(text) : null; } catch { json = text; }
  return { status: response.status, json };
}

async function must(base, method, path, body) {
  const result = await api(base, method, path, body);
  if (result.status >= 300) throw new Error(`${method} ${path} -> ${result.status} ${JSON.stringify(result.json).slice(0, 400)}`);
  return result.json;
}

async function waitFor(label, seconds, probe) {
  const deadline = Date.now() + seconds * 1000;
  let last;
  while (Date.now() < deadline) {
    try {
      last = await probe();
      if (last?.ok) return last.value;
    } catch (error) {
      last = { seen: String(error.message ?? error) };
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(`${label}: not within ${seconds}s; last seen ${JSON.stringify(last?.seen ?? last).slice(0, 400)}`);
}

function pass(name) { console.log(`PASS ${elapsed().padStart(7)}  ${name}`); }

/** Text drawn by output (type 1, 9-byte header) and snapshot (type 2, 14-byte header) frames. */
function frameText(data) {
  const frame = new Uint8Array(data);
  if (frame[0] === 1) return Buffer.from(frame.slice(9)).toString("utf8");
  if (frame[0] === 2) return Buffer.from(frame.slice(14)).toString("utf8");
  return "";
}

function openSocket(url, protocol) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url, [protocol]);
    socket.binaryType = "arraybuffer";
    const shown = { text: "", closed: false };
    socket.addEventListener("message", (event) => { if (event.data instanceof ArrayBuffer) shown.text += frameText(event.data); });
    socket.addEventListener("close", () => { shown.closed = true; });
    socket.addEventListener("open", () => resolve({ socket, shown }), { once: true });
    socket.addEventListener("error", () => reject(new Error(`WebSocket to ${url} failed`)), { once: true });
  });
}

async function health(base) {
  return waitFor(`${base} healthy`, 60, async () => {
    const response = await fetch(`${base}/health`);
    return { ok: response.ok, value: await response.json(), seen: response.status };
  });
}

async function main() {
  const [, memberHealth] = await Promise.all([health(KEEPER), health(MEMBER)]);
  pass(`both Hives are up (member ${memberHealth.version})`);

  // ── Join, entirely over HTTP, the way the two control rooms do it ──
  await must(KEEPER, "POST", "/api/v1/apiary", { name: "Two Hive Garden", shared_work_backend: "jira" });
  const card = await must(MEMBER, "GET", "/api/v1/apiary/connection-card");
  const memberHive = card.payload.hive_id;
  await must(KEEPER, "POST", "/api/v1/apiary/hive-candidates", card);
  const bundle = await must(KEEPER, "POST", `/api/v1/apiary/hive-candidates/${memberHive}/invitation`);
  const invitation = await must(MEMBER, "POST", "/api/v1/apiary/join-invitations", bundle);
  const invitationId = invitation.id ?? invitation.invitation_id;
  const revision = invitation.policy?.revision ?? invitation.policy_revision ?? 1;
  await must(MEMBER, "POST", `/api/v1/apiary/join-invitations/${invitationId}/policy-acceptance`, { policy_revision: revision });
  await must(MEMBER, "POST", `/api/v1/apiary/join-invitations/${invitationId}/submission`);
  pass("the member joined the Keeper's Apiary over HTTP");

  // ── The member's Queen, running in its own terminal engine ──
  const workers = await must(MEMBER, "GET", "/api/v1/workers");
  const queen = (Array.isArray(workers) ? workers : workers.workers).find((worker) => worker.role === "queen");
  if (!queen.running) await must(MEMBER, "POST", `/api/v1/workers/${queen.id}/start`, { rows: 24, columns: 80 });
  pass("the member's Queen is running");

  // ── 1. The Keeper learns which release the member runs ──
  await waitFor("the member's version report reaches the Keeper", 90, async () => {
    const fleet = await must(KEEPER, "GET", "/api/v1/apiary/fleet-versions");
    const hive = fleet.hives.find((entry) => entry.hive_id === memberHive);
    return { ok: hive?.swarm_version === memberHealth.version, seen: hive ?? fleet };
  });
  pass("the Keeper sees the member's release");

  // ── 2. A Keeper restart must not leave the member unable to sync ──
  execFileSync("systemctl", ["--user", "stop", KEEPER_UNIT]);
  await must(MEMBER, "POST", "/api/v1/apiary/sync-retry");
  const failed = await waitFor("the member tries to sync while the Keeper is down", 60, async () => {
    const sync = await must(MEMBER, "GET", "/api/v1/apiary/sync-health");
    return { ok: !["idle", "current"].includes(sync.condition), value: sync, seen: sync };
  });
  console.log(`      while the Keeper was down the member recorded: ${failed.condition}`);
  execFileSync("sh", [KEEPER_START]);
  await health(KEEPER);
  await waitFor("the member syncs again after the Keeper is back", 150, async () => {
    const sync = await must(MEMBER, "GET", "/api/v1/apiary/sync-health");
    return { ok: sync.condition === "current" && (sync.last_success_at ?? 0) * 1000 > started, seen: sync };
  });
  pass("the member recovers by itself after the Keeper restarts behind a proxy");

  // ── 2b. One refused request must not silence the member for good ──
  // A 409 on one step is what "incompatible" was made of, and it used to halt
  // every later sync — version report, watch and takeover acceptance with it.
  fs.writeFileSync(PROXY_FAULT, "/api/v1/federation/catalog 409\n");
  await must(MEMBER, "POST", "/api/v1/apiary/sync-retry");
  const refused = await waitFor("the member records the refused step", 60, async () => {
    const sync = await must(MEMBER, "GET", "/api/v1/apiary/sync-health");
    return { ok: sync.condition === "incompatible", value: sync, seen: sync };
  });
  console.log(`      a refused catalog request recorded: ${refused.condition} at "${refused.failed_step ?? "(no step recorded)"}"`);
  fs.rmSync(PROXY_FAULT);
  await waitFor("the member syncs again once the refusal stops", 150, async () => {
    const sync = await must(MEMBER, "GET", "/api/v1/apiary/sync-health");
    return { ok: sync.condition === "current", seen: sync };
  });
  pass("one refused request does not stop the member synchronising");

  // ── 2c. The Keeper can tell when a member last reached it ──
  const members = await must(KEEPER, "GET", "/api/v1/apiary/members");
  const seen = (Array.isArray(members) ? members : members.members ?? []).find((member) => member.hive_id === memberHive);
  if (!seen?.last_contact_at || Date.now() / 1000 - seen.last_contact_at > 120) {
    throw new Error(`the Keeper does not show when the member last reached it: ${JSON.stringify(seen)}`);
  }
  pass("the Keeper shows when the member last reached it");

  // ── 3. Watching shows the member's screen ──
  const watch = await must(KEEPER, "POST", "/api/v1/apiary/watches", { target_hive_id: memberHive });
  await waitFor("the member acknowledges the watch", 60, async () => {
    const audit = await must(KEEPER, "GET", "/api/v1/apiary/watches");
    const entry = (Array.isArray(audit) ? audit : audit.watches ?? []).find((item) => (item.id ?? item.watch?.id) === watch.id);
    const acknowledged = entry?.acknowledged_at ?? entry?.watch?.acknowledged_at;
    return { ok: Boolean(acknowledged), seen: entry };
  });
  const watchGrant = await must(KEEPER, "POST", `/api/v1/apiary/watches/${watch.id}/grant`);
  const viewer = await openSocket(`${KEEPER.replace("http", "ws")}${watchGrant.websocket_path}`, `swarm-watch.${watchGrant.grant}`);
  await waitFor("the watch window shows the member's Queen", 60, async () => ({ ok: viewer.shown.text.includes("two-hive-queen ready"), seen: viewer.shown.text.slice(-200) }));
  viewer.socket.close();
  pass("watching shows the member's screen");

  // A window opened after the member already sent its screen, onto a screen
  // that has not changed since. The relay keeps no frames, so this one sees
  // anything only if it can ask for the screen; the member's own resend comes
  // a minute later, which is what "never gets past waiting" looked like.
  await new Promise((resolve) => setTimeout(resolve, 3000));
  const lateGrant = await must(KEEPER, "POST", `/api/v1/apiary/watches/${watch.id}/grant`);
  const late = await openSocket(`${KEEPER.replace("http", "ws")}${lateGrant.websocket_path}`, `swarm-watch.${lateGrant.grant}`);
  await waitFor("a window opened later shows the member's Queen", 10, async () => ({ ok: late.shown.text.includes("two-hive-queen ready"), seen: { closed: late.shown.closed, text: late.shown.text.slice(-200) } }));
  late.socket.close();
  await must(KEEPER, "DELETE", `/api/v1/apiary/watches/${watch.id}`);
  pass("a window opened later shows the screen at once");

  // ── 4. Takeover: the screen, typing into it, and taking it back ──
  const lease = await must(KEEPER, "POST", "/api/v1/apiary/takeovers", { target_hive_id: memberHive, reason: "Two-Hive acceptance" });
  const leaseId = lease.id ?? lease.lease?.id;
  // Exactly what the takeover window does: ask for a ticket until one is given,
  // then attach. A ticket given before the member accepts is the bug the
  // operator saw as "The takeover ended" a second after asking.
  const controlGrant = await waitFor("the member accepts the takeover", 60, async () => {
    const result = await api(KEEPER, "POST", `/api/v1/apiary/takeovers/${leaseId}/control-grant`);
    return { ok: result.status < 300, value: result.json, seen: result };
  });
  const control = await openSocket(`${KEEPER.replace("http", "ws")}${controlGrant.websocket_path}`, `swarm-takeover.${controlGrant.grant}`);
  await waitFor("the takeover window shows the member's Queen", 60, async () => ({ ok: control.shown.text.includes("two-hive-queen ready"), seen: { closed: control.shown.closed, text: control.shown.text.slice(-200) } }));
  pass("the takeover window shows the member's screen");
  const keystroke = Buffer.concat([Buffer.from([9]), Buffer.from("hello-from-keeper\n")]);
  await waitFor("what the Keeper types reaches the member's terminal", 30, async () => {
    if (!control.shown.text.includes("typed:hello-from-keeper")) control.socket.send(keystroke);
    return { ok: control.shown.text.includes("typed:hello-from-keeper"), seen: { closed: control.shown.closed, text: control.shown.text.slice(-200) } };
  });
  pass("typing in the takeover window reaches the member's terminal");
  await must(MEMBER, "POST", `/api/v1/apiary/takeovers/${leaseId}/reclaim`, { reason: "Two-Hive acceptance: taking it back" });
  await waitFor("the Keeper sees the member took it back", 60, async () => {
    const audit = await must(KEEPER, "GET", "/api/v1/apiary/takeover-audit");
    const entry = (Array.isArray(audit) ? audit : audit.entries ?? []).find((item) => (item.lease?.id ?? item.id) === leaseId);
    const state = entry?.lease?.state ?? entry?.state;
    return { ok: state === "reclaimed", seen: entry };
  });
  control.socket.close();
  pass("the member takes it back and the Keeper records it");

  // ── 5. The same takeover from a real Keeper browser, typed on a keyboard ──
  // Everything above speaks the wire protocol directly. The operator does not:
  // they click Take over and type into an xterm, and on 2026-09-29 that is the
  // path where keystrokes went nowhere while every check above passed.
  const memberName = (await must(KEEPER, "GET", "/api/v1/apiary/members")).find((entry) => entry.hive_id === memberHive).hive_name;
  const { chromium } = require("playwright");
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1366, height: 900 } });
  try {
    page.on("dialog", (dialog) => void dialog.accept("Two-Hive acceptance, from a browser"));
    await page.goto(KEEPER);
    // A browser on the Hive's own machine may already be trusted; unlock only
    // when asked.
    const tokenInput = page.getByLabel("Operator token");
    const apiary = page.getByRole("button", { name: /^Apiary/ }).first();
    await tokenInput.or(apiary).first().waitFor({ timeout: 20_000 });
    if (await tokenInput.isVisible()) {
      await tokenInput.fill(TOKEN);
      await page.getByRole("button", { name: "Unlock Swarm" }).click();
    }
    await apiary.click();
    // The member's operator has Queen open in their own browser, as they did in
    // the field. Their page tries to take its terminal back while it is held,
    // and a test with nobody at the member cannot see what that does.
    const memberPage = await browser.newPage({ viewport: { width: 1366, height: 900 } });
    await memberPage.goto(MEMBER);
    const memberToken = memberPage.getByLabel("Operator token");
    const memberWorkers = memberPage.getByRole("button", { name: /^Workers/ }).first();
    await memberToken.or(memberWorkers).first().waitFor({ timeout: 20_000 });
    if (await memberToken.isVisible()) {
      await memberToken.fill(TOKEN);
      await memberPage.getByRole("button", { name: "Unlock Swarm" }).click();
    }
    await memberWorkers.click();
    // Drawn on a canvas, so the screen is not readable here; its status is.
    await memberPage.waitForFunction(() => /\bConnected\b/.test(document.body.innerText), undefined, { timeout: 30_000 });
    console.log(`      ${elapsed()} the member's browser shows Queen`);
    const row = page.getByRole("list", { name: "Keeper Apiary Hives" }).getByRole("listitem").filter({ hasText: memberName });
    await row.getByRole("button", { name: "Take over" }).click();
    const held = page.getByRole("dialog", { name: `Controlling ${memberName}` });
    console.log(`      ${elapsed()} Take over clicked`);
    await held.getByText("Live — you are typing on this Hive").waitFor({ timeout: 60_000 });
    console.log(`      ${elapsed()} the takeover window is live`);
    await waitFor("the browser takeover window shows the member's Queen", 30, async () => {
      const text = await held.locator(".xterm-rows").innerText();
      return { ok: text.includes("two-hive-queen ready"), seen: text.slice(-200) };
    });
    pass("a Keeper browser opens the takeover window on the member's screen");
    await held.locator(".takeover-window-surface").click();
    await page.keyboard.type("hello-from-browser");
    await page.keyboard.press("Enter");
    await waitFor("what the Keeper types in the browser reaches the member's terminal", 30, async () => {
      const text = await held.locator(".xterm-rows").innerText();
      const status = await held.getByRole("status").innerText();
      return { ok: text.includes("typed:hello-from-browser"), seen: { status, text: text.slice(-300) } };
    });
    pass("typing in the browser's takeover window reaches the member's terminal");

    // ── 5b. Handing back keeps the Keeper watching, and returns the member's terminal at once ──
    // The live check on 2026-10-03: handing back closed the window, WSL took a
    // while to notice, and WSL's own operator could neither type nor resume —
    // not even after a hard refresh — until the lease lapsed five minutes later.
    await held.getByRole("button", { name: "Hand back" }).click();
    const handedBack = Date.now();
    const resume = memberPage.getByRole("button", { name: "Resume Here" });
    let resumed = false;
    await waitFor("the member's page stops showing it is held", 20, async () => {
      if (await resume.isVisible().catch(() => false)) { resumed = true; await resume.click().catch(() => undefined); }
      const text = await memberPage.locator("body").innerText();
      const locked = /takeover authority is missing|Someone else is controlling this Hive/.test(text);
      const viewingOnly = await resume.isVisible().catch(() => false);
      return { ok: !locked && !viewingOnly, seen: text.replace(/\s+/g, " ").slice(0, 300) };
    });
    // Not a pass on its own: a page with no lock showing is not yet a terminal
    // that takes input. The typing check below is the proof.
    console.log(`      ${elapsed()} the member's page shows no lock${resumed ? " (after pressing Resume Here)" : ""}`);
    const watchingAgain = page.getByRole("dialog", { name: `Live window into ${memberName}` });
    await watchingAgain.waitFor({ timeout: 15_000 });
    pass("handing back leaves the Keeper watching the member");
    await memberPage.locator(".terminal-surface:visible").first().click();
    await memberPage.keyboard.type("hello-from-member");
    await memberPage.keyboard.press("Enter");
    await waitFor("what the member types after hand-back reaches its terminal", 20, async () => {
      const text = await watchingAgain.locator(".xterm-rows").innerText().catch(() => "");
      return { ok: text.includes("typed:hello-from-member"), seen: text.slice(-200) };
    });
    pass(`what the member types reaches its own terminal ${((Date.now() - handedBack) / 1000).toFixed(1)}s after hand-back`);
    await watchingAgain.getByRole("button", { name: "Stop watching" }).click();
  } catch (error) {
    // A browser failure without a picture sends someone off to reproduce it by
    // hand, which is the leg work this run exists to remove.
    fs.mkdirSync(ARTIFACTS, { recursive: true });
    const pages = browser.contexts().flatMap((context) => context.pages());
    const seen = [];
    for (const [index, open] of pages.entries()) {
      const name = `${ARTIFACTS}/browser-failure-${index + 1}`;
      await open.screenshot({ path: `${name}.png`, fullPage: true }).catch(() => undefined);
      const text = await open.locator("body").innerText().catch(() => "(page text unavailable)");
      fs.writeFileSync(`${name}.txt`, `${open.url()}\n\n${text}`);
      seen.push(`${open.url()}: ${text.replace(/\s+/g, " ").slice(0, 240)}`);
    }
    throw new Error(`${error.message}\n      ${seen.join("\n      ")}\n      saved: ${ARTIFACTS}/browser-failure-*.png`);
  } finally {
    await browser.close();
  }

  // ── 6. The Keeper can read the member's own log (ADR 0112) ──
  // What made this run useful — the member's account of what went wrong — has
  // to reach the Keeper by itself, not only this run's copy of the journal.
  await waitFor("the member's log reaches the Keeper", 60, async () => {
    const log = await must(KEEPER, "GET", `/api/v1/apiary/hives/${memberHive}/diagnostics`);
    const stopped = log.entries.some((entry) => entry.message.includes("synchronisation stopped at this step"));
    const relayed = log.entries.some((entry) => entry.message.includes("takeover relay connected to Keeper"));
    return { ok: stopped && relayed, seen: { lines: log.entries.length, received_at: log.received_at, stopped, relayed } };
  });
  pass("the Keeper can read the member's own log, including why it stopped");

  console.log(`\nALL PASSED in ${elapsed()}`);
}

main().catch((error) => {
  console.error(`FAIL ${elapsed().padStart(7)}  ${error.message}`);
  process.exit(1);
});
