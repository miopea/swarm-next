#!/usr/bin/env node

/**
 * A FICTIONAL Admin, so the paired support tests can actually be run.
 *
 * ⚠️ WHY THIS EXISTS. crates/swarm-api/src/support_paired_test.rs carries two
 * #[ignore]d tests whose reason reads "requires an explicitly started fictional
 * Admin loopback fixture". No such fixture was in this repository, and the two
 * environment variables they read — SWARM_TEST_ADMIN_FEEDBACK_ENDPOINT and
 * SWARM_TEST_ADMIN_ATTACHMENTS_ENDPOINT — appeared nowhere else in the tree. So
 * the handoff's claim that "paired route recovery now passes" could not be
 * reproduced by anyone reading this repo, and the tests never run in verify.sh.
 * This makes that claim checkable instead of inherited.
 *
 * ⚠️ WHAT IT IS NOT. Not BFG Admin, not a client of it, and not production. It
 * binds loopback only, invents its own receipt identities, and stores nothing
 * beyond the life of the process. Admin's real bounded attachment route is
 * ADR 0080 work that is still in progress on their side; this implements the
 * agreed contract so Swarm's half can be exercised, and proves nothing about
 * theirs. Activating production attachment intake remains separately barred.
 *
 * Contract honoured, taken from the tests rather than assumed:
 *   - first submission under a key    -> receipt, deduplicated false
 *   - identical replay of that key    -> THE SAME receipt, deduplicated true
 *   - different content, same key     -> 409, and the original survives intact
 *
 * Usage:
 *   node scripts/dogfood/admin-support-fixture.cjs            # prints both endpoints
 *   node scripts/dogfood/admin-support-fixture.cjs --port 8123
 */

const http = require("node:http");
const crypto = require("node:crypto");

const TEXT_PATH = "/api/feedback/swarm-support/submissions";
const FILES_PATH = "/api/feedback/swarm-support/submissions-with-attachments";
const MAX_BODY_BYTES = 32 * 1024 * 1024;

const portArgument = process.argv.indexOf("--port");
const port = portArgument === -1 ? 0 : Number(process.argv[portArgument + 1]);

/** submission_key -> { fingerprint, receipt }. Process-lifetime only, by design. */
const issued = new Map();

function readBody(request) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    request.on("data", (chunk) => {
      size += chunk.length;
      if (size > MAX_BODY_BYTES) {
        reject(new Error("body too large"));
        request.destroy();
        return;
      }
      chunks.push(chunk);
    });
    request.on("end", () => resolve(Buffer.concat(chunks)));
    request.on("error", reject);
  });
}

/**
 * The submission key, and a fingerprint of everything that must not change
 * under it.
 *
 * For the multipart route the manifest part carries both the submission and the
 * attachment metadata, and ADR 0080 makes metadata and manifest ORDER part of
 * what a retry may not alter — so the raw manifest bytes are the fingerprint,
 * not a re-serialisation of them.
 */
function identify(body, isMultipart) {
  const text = body.toString("utf8");
  if (!isMultipart) {
    const submission = JSON.parse(text);
    return { key: submission.submission_key, fingerprint: text };
  }
  const manifest = text.slice(text.indexOf("{"), text.lastIndexOf("}") + 1);
  return { key: JSON.parse(manifest).submission.submission_key, fingerprint: manifest };
}

const server = http.createServer(async (request, response) => {
  const path = (request.url ?? "/").split("?")[0];
  if (request.method !== "POST" || (path !== TEXT_PATH && path !== FILES_PATH)) {
    response.writeHead(404).end();
    return;
  }
  let identity;
  try {
    identity = identify(await readBody(request), path === FILES_PATH);
  } catch {
    response.writeHead(400).end();
    return;
  }
  if (!identity.key) {
    response.writeHead(422).end();
    return;
  }

  const existing = issued.get(identity.key);
  if (existing) {
    // A retry that changed anything is refused, and REFUSING MUST NOT DESTROY
    // the original — the test asserts the first receipt still replays after a
    // conflict, which is the part a naive implementation gets wrong by
    // overwriting the entry it just rejected.
    if (existing.fingerprint !== identity.fingerprint) {
      response.writeHead(409).end();
      return;
    }
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ ...existing.receipt, deduplicated: true }));
    return;
  }

  const receipt = {
    submission_key: identity.key,
    // ⚠️ BARE UUIDs, NOT PREFIXED STRINGS. swarm-persistence validates both with
    // `Uuid::parse_str(..).is_ok_and(|id| !id.is_nil())`, so a friendly
    // "fictional-conversation-<uuid>" is rejected — and it is rejected QUIETLY:
    // settle falls back from Confirmed to Uncertain rather than erroring, so the
    // only symptom is a delivery that never confirms. Randomness is what makes
    // these fictional; a prefix is not worth an unexplainable failure.
    conversation_id: crypto.randomUUID(),
    message_id: crypto.randomUUID(),
    created_at: Math.floor(Date.now() / 1000),
    deduplicated: false,
  };
  issued.set(identity.key, { fingerprint: identity.fingerprint, receipt });
  response.writeHead(201, { "content-type": "application/json" });
  response.end(JSON.stringify(receipt));
});

// Loopback only. The tests assert the host is 127.0.0.1 and refuse anything
// else, and binding wider would make this reachable from off the machine.
server.listen(port, "127.0.0.1", () => {
  const bound = server.address().port;
  console.log(`fictional Admin listening on 127.0.0.1:${bound}`);
  console.log(`SWARM_TEST_ADMIN_FEEDBACK_ENDPOINT=http://127.0.0.1:${bound}${TEXT_PATH}`);
  console.log(`SWARM_TEST_ADMIN_ATTACHMENTS_ENDPOINT=http://127.0.0.1:${bound}${FILES_PATH}`);
});
