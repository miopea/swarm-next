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
 * ⚠️ WHAT PRODUCTION ACTUALLY ENFORCES, so a green run here means something.
 *
 * ⚠️ AND THE FIRST VERSION OF THIS COMMENT WAS WRONG IN THE EXACT WAY IT WARNS
 * ABOUT. It said BFG Admin "probed their live route (build 245240d)". They did
 * not: that answer came from READING THEIR SHIPPED CODE, under a heading saying
 * "the contract, as implemented", and I upgraded it to "probed" in my own
 * retelling. They retracted it themselves on 2026-09-18 and probed properly
 * against build 7180274. A code-read repeated as a probe is the same defect as a
 * loose fixture — confidence without the measurement that would earn it.
 *
 * Their warning is still worth repeating: a fixture looser than production is
 * worse than no fixture, because it manufactures confidence and moves the
 * failure to the one environment nobody is testing in.
 *
 * Swarm's own validate_support_attachment_set agrees with every one of these
 * (four types, 4 files, 5 MiB each, 12 MiB total, sha256), checked rather than
 * assumed, so this is the contract on BOTH sides rather than Admin's alone.
 */
const ALLOWED_MEDIA_TYPES = new Set(["image/png", "image/jpeg", "image/webp", "text/plain"]);
const MAX_FILES = 4;
const MAX_FILE_BYTES = 5 * 1024 * 1024;
const MAX_FILES_BYTES = 12 * 1024 * 1024;
const SHA256 = /^[a-f0-9]{64}$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** A refusal carrying the status production would return. */
class Refused extends Error {
  constructor(status, detail) {
    super(detail);
    this.status = status;
  }
}

/** Splits a multipart body into ordered parts, because ORDER is part of the contract. */
function parts(body, contentType) {
  const marker = /boundary=(?:"([^"]+)"|([^;]+))/i.exec(contentType ?? "");
  if (!marker) throw new Refused(400, "no multipart boundary");
  const boundary = Buffer.from(`--${marker[1] ?? marker[2]}`);
  const found = [];
  let index = body.indexOf(boundary);
  while (index !== -1) {
    const start = index + boundary.length;
    if (body.slice(start, start + 2).toString() === "--") break;
    const next = body.indexOf(boundary, start);
    const chunk = body.slice(start, next === -1 ? body.length : next);
    const split = chunk.indexOf("\r\n\r\n");
    if (split !== -1) {
      const headers = chunk.slice(0, split).toString("utf8");
      found.push({
        name: /name="([^"]*)"/i.exec(headers)?.[1] ?? "",
        contentType: /content-type:\s*([^\r\n;]+)/i.exec(headers)?.[1]?.trim() ?? "",
        body: chunk.slice(split + 4, chunk.length - 2),
      });
    }
    index = next;
  }
  return found;
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
function identify(body, isMultipart, contentType) {
  if (!isMultipart) {
    const text = body.toString("utf8");
    return { key: JSON.parse(text).submission_key, fingerprint: text };
  }
  const found = parts(body, contentType);
  // FIRST part must be the manifest. Production answers a body without one with
  // 400 "Missing manifest", which is how the route was proved registered at all.
  const [first, ...files] = found;
  if (!first || first.name !== "manifest") throw new Refused(400, "Missing manifest");
  if (!first.contentType.startsWith("application/json")) throw new Refused(400, "manifest is not json");
  const manifestText = first.body.toString("utf8");
  const manifest = JSON.parse(manifestText);
  const declared = manifest.attachments ?? [];

  if (declared.length > MAX_FILES) throw new Refused(400, `more than ${MAX_FILES} attachments`);
  if (files.length !== declared.length) throw new Refused(400, "file parts do not match the manifest");

  // ⚠️ THE CLOSED SET IS CHECKED HERE, WITH THE MANIFEST, AND ANSWERS 400.
  //
  // This fixture said 415 and that was WRONG. Corrected 2026-09-18 from BFG
  // Admin's own probe of production build 7180274, after they retracted their
  // earlier answer: that one came from READING THEIR SHIPPED CODE, and I
  // upgraded it to "probed" in my own retelling. It was never a probe.
  //
  // Probed, both cases, production:
  //   manifest declares image/heic  -> 400 {"error":"Invalid attachment submission"}
  //   manifest says png, part is text/plain
  //                                 -> 415 {"error":"File media type differs from manifest"}
  //
  // The reason is structural and is why the ORDER matters as much as the code:
  // media_type is a zod enum INSIDE the manifest, so an unknown value fails the
  // manifest parse and lands in the generic ZodError branch BEFORE any part is
  // examined. Checking it per-part, as this fixture did, could answer 415 for a
  // submission production rejects at 400 without ever looking at the parts.
  for (const entry of declared) {
    if (!ALLOWED_MEDIA_TYPES.has(entry.media_type)) {
      throw new Refused(400, `media_type ${entry.media_type} is outside the accepted set`);
    }
  }

  let total = 0;
  for (const [position, entry] of declared.entries()) {
    if (!UUID.test(entry.id ?? "")) throw new Refused(400, "attachment id is not a uuid");
    if (!SHA256.test(entry.sha256 ?? "")) throw new Refused(400, "sha256 must be 64 lowercase hex");
    const part = files[position];
    if (part.name !== `file:${entry.id}`) throw new Refused(400, "file part is not named for its manifest id");
    // ⚠️ 415 IS EXCLUSIVELY THIS CASE — the manifest's claim about the bytes
    // versus what actually arrived. Production checks it twice, at the part
    // header and by sniffing the bytes. It is NOT the unsupported-type code.
    if (part.contentType !== entry.media_type) {
      throw new Refused(415, `part says ${part.contentType}, manifest says ${entry.media_type}`);
    }
    if (part.body.length > MAX_FILE_BYTES) throw new Refused(413, "attachment over 5 MiB");
    total += part.body.length;
  }
  if (total > MAX_FILES_BYTES) throw new Refused(413, "attachments over 12 MiB in total");

  return { key: manifest.submission.submission_key, fingerprint: manifestText };
}

const server = http.createServer(async (request, response) => {
  const path = (request.url ?? "/").split("?")[0];
  if (request.method !== "POST" || (path !== TEXT_PATH && path !== FILES_PATH)) {
    response.writeHead(404).end();
    return;
  }
  let identity;
  try {
    identity = identify(
      await readBody(request),
      path === FILES_PATH,
      request.headers["content-type"],
    );
  } catch (error) {
    // The status production would have returned, so a caller sees the same
    // refusal here as there rather than a generic 400 standing in for all of them.
    response.writeHead(error instanceof Refused ? error.status : 400).end();
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
