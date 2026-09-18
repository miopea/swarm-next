#!/usr/bin/env node

/**
 * Drives the fictional Admin fixture, so the contract it encodes is CHECKED.
 *
 * ⚠️ WHY THIS EXISTS: THE FIXTURE HAD NEVER ONCE BEEN RUN BY A CHECK. It was
 * added on 2026-09-17 for the two #[ignore]d tests in support_paired_test.rs,
 * which verify.sh does not run, and its contract was proved by hand exactly
 * once, by the person who wrote it, on the day they wrote it. `pnpm test:dogfood`
 * runs `scripts/dogfood/*.test.cjs`; the fixture is not a `.test.cjs` and nothing
 * required it. So the agreed contract was written down and then left unguarded.
 *
 * That is the same shape as a coordinator detector whose only callers sit in
 * #[cfg(test)] — implemented, never exercised, and silent in a way that reads
 * like health. This file is what makes the fixture's claims falsifiable.
 *
 * ⚠️ AND THE CONTRACT IT NOW ASSERTS IS THE CORRECTED ONE. Until 2026-09-18 the
 * fixture answered 415 for a media type outside the accepted four. Production
 * answers 400: media_type is a zod enum inside the manifest, so an unknown value
 * fails the manifest parse before any part is examined. 415 is EXCLUSIVELY the
 * declared-versus-actual mismatch. Probed by BFG Admin against production build
 * 7180274, after they retracted an earlier answer that had been read from code
 * rather than measured.
 */

const { test } = require("node:test");
const assert = require("node:assert/strict");
const { spawn } = require("node:child_process");
const path = require("node:path");
const crypto = require("node:crypto");

const FIXTURE = path.join(__dirname, "admin-support-fixture.cjs");
const BOUNDARY = "----swarmfixtureboundary";

/** Starts the fixture and resolves the attachments endpoint it prints. */
function startFixture() {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [FIXTURE], { stdio: ["ignore", "pipe", "pipe"] });
    const failed = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error("fixture did not announce an endpoint within 10s"));
    }, 10_000);
    let seen = "";
    child.stdout.on("data", (chunk) => {
      seen += chunk.toString("utf8");
      const found = seen.match(/SWARM_TEST_ADMIN_ATTACHMENTS_ENDPOINT=(\S+)/);
      if (found) {
        clearTimeout(failed);
        resolve({ endpoint: found[1], child });
      }
    });
    child.on("error", (error) => {
      clearTimeout(failed);
      reject(error);
    });
  });
}

/** One multipart body. `parts` are [name, contentType, body] in order. */
function multipart(parts) {
  const chunks = [];
  for (const [name, contentType, body] of parts) {
    chunks.push(
      Buffer.from(
        `--${BOUNDARY}\r\nContent-Disposition: form-data; name="${name}"\r\n` +
          `Content-Type: ${contentType}\r\n\r\n`,
        "utf8",
      ),
      Buffer.isBuffer(body) ? body : Buffer.from(body, "utf8"),
      Buffer.from("\r\n", "utf8"),
    );
  }
  chunks.push(Buffer.from(`--${BOUNDARY}--\r\n`, "utf8"));
  return Buffer.concat(chunks);
}

function attachment(mediaType, bytes) {
  const body = Buffer.from(bytes, "utf8");
  return {
    id: crypto.randomUUID(),
    media_type: mediaType,
    sha256: crypto.createHash("sha256").update(body).digest("hex"),
    body,
  };
}

/** Posts a manifest plus its parts, with the part types optionally lying. */
async function post(endpoint, entries, { partTypes, manifestLast = false, omitManifest = false } = {}) {
  const manifest = JSON.stringify({
    submission: {
      // The fixture reads manifest.submission.submission_key as the dedup
      // identity and answers 422 without one — so every call needs a fresh key,
      // or the second would be a replay of the first rather than a new case.
      submission_key: crypto.randomUUID(),
      message: "Fictional acceptance only. No customer data.",
    },
    attachments: entries.map(({ id, media_type, sha256 }) => ({ id, media_type, sha256 })),
  });
  const files = entries.map((entry, index) => [
    `file:${entry.id}`,
    partTypes?.[index] ?? entry.media_type,
    entry.body,
  ]);
  let parts;
  if (omitManifest) parts = files;
  else if (manifestLast) parts = [...files, ["manifest", "application/json", manifest]];
  else parts = [["manifest", "application/json", manifest], ...files];

  const response = await fetch(endpoint, {
    method: "POST",
    headers: { "content-type": `multipart/form-data; boundary=${BOUNDARY}` },
    body: multipart(parts),
  });
  return response.status;
}

test("the fictional Admin enforces the contract Swarm was told production enforces", async (t) => {
  const { endpoint, child } = await startFixture();
  t.after(() => child.kill("SIGKILL"));

  assert.match(endpoint, /^http:\/\/127\.0\.0\.1:\d+\//, "loopback only, never a wider bind");

  await t.test("an accepted submission is created", async () => {
    const status = await post(endpoint, [attachment("text/plain", "hello")]);
    assert.equal(status, 201);
  });

  // ⚠️ THE CASE THIS FIXTURE GOT WRONG FOR A DAY. An unsupported type is 400,
  // not 415, because media_type is a zod enum inside the manifest and an unknown
  // value fails the manifest parse before any part is looked at.
  await t.test("a media type outside the four is 400, not 415", async () => {
    const status = await post(endpoint, [attachment("image/heic", "not really heic")]);
    assert.equal(status, 400, "an unsupported type fails the MANIFEST, before any part is read");
  });

  // ⚠️ AND THIS IS THE ONLY 415. Asserted beside the case above on purpose: the
  // two were conflated, and only asserting them together shows they are distinct.
  await t.test("a part whose actual type contradicts the manifest is 415", async () => {
    const entry = attachment("image/png", "pretending to be a png");
    const status = await post(endpoint, [entry], { partTypes: ["text/plain"] });
    assert.equal(status, 415, "415 is exclusively declared-versus-actual mismatch");
  });

  await t.test("the manifest must come first", async () => {
    const status = await post(endpoint, [attachment("text/plain", "hello")], { manifestLast: true });
    assert.equal(status, 400);
  });

  await t.test("a submission with no manifest is refused", async () => {
    const status = await post(endpoint, [attachment("text/plain", "hello")], { omitManifest: true });
    assert.equal(status, 400);
  });
});
