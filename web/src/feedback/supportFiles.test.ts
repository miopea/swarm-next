import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { IDBFactory } from "fake-indexeddb";
import { loadPendingSupportFiles, savePendingSupportFiles, clearPendingSupportFiles, validateSupportFiles, prepareSupportFiles } from "./supportFiles";
import type { SupportFileReport } from "../api/support";

beforeEach(async () => { vi.stubGlobal("indexedDB", new IDBFactory()); vi.stubGlobal("crypto", (await vi.importActual<{ webcrypto: Crypto }>("node:crypto")).webcrypto); });
afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });
async function report(): Promise<SupportFileReport> {
  const bytes = new TextEncoder().encode("fictional bytes").buffer as ArrayBuffer;
  const sha256 = [...new Uint8Array(await crypto.subtle.digest("SHA-256",bytes))].map((n) => n.toString(16).padStart(2,"0")).join("");
  return { submission: { submission_key: crypto.randomUUID(), kind: "bug_report", email: "fictional@example.invalid", name: null, subject: "Fixture", body: "Reviewed words" },
    files: [{ metadata: { id: crypto.randomUUID(), file_name: "fictional.txt", media_type: "text/plain", size_bytes: bytes.byteLength, sha256 }, bytes }] };
}

/**
 * ⚠️ THREE DIFFERENT PROBLEMS MUST NOT SHARE ONE SENTENCE.
 *
 * They used to. An oversized PNG was answered with "Use nonempty PNG, JPEG,
 * WebP or plain text files" — naming the person's own format back at them as
 * the remedy for a problem about SIZE. That reads as though the product cannot
 * recognise a PNG, and it sends them to the wrong fix.
 */
test("an oversized file is told about its size, not about its format", async () => {
  const big = new File([new Uint8Array(1)], "screenshot.png", { type: "image/png" });
  Object.defineProperty(big, "size", { value: 6 * 1024 * 1024 });

  await expect(prepareSupportFiles([big])).rejects.toThrow(/larger than 5 MiB/);
  // The format was never the problem, so it must not be offered as the answer.
  await expect(prepareSupportFiles([big])).rejects.toThrow(/the format is fine/);
  await expect(prepareSupportFiles([big])).rejects.toThrow(/screenshot\.png/);
});

test("a phone photo is told what it actually is, and what would work instead", async () => {
  // HEIC is what an iPhone camera produces by default, and `accept` is a hint
  // rather than a guard on a phone — so this is the likeliest way to land here.
  const photo = new File([new Uint8Array([1, 2, 3])], "IMG_0001.HEIC", { type: "image/heic" });

  await expect(prepareSupportFiles([photo])).rejects.toThrow(/image\/heic/);
  await expect(prepareSupportFiles([photo])).rejects.toThrow(/PNG, JPEG, WebP or plain text/);
  await expect(prepareSupportFiles([photo])).rejects.toThrow(/IMG_0001\.HEIC/);
});

test("a file the browser gives no type for is not described as empty", async () => {
  // An empty `type` and an empty FILE are different failures, and the message
  // for one must not be handed to the other.
  const odd = new File([new Uint8Array([1])], "notes.sketch", { type: "" });

  await expect(prepareSupportFiles([odd])).rejects.toThrow(/unrecognised format/);
  await expect(prepareSupportFiles([odd])).rejects.not.toThrow(/is empty/);
});

test("an empty file says so, rather than listing formats", async () => {
  const empty = new File([], "nothing.png", { type: "image/png" });

  await expect(prepareSupportFiles([empty])).rejects.toThrow(/nothing\.png is empty/);
});

test("atomic byte copies survive reopen and exact retry without rereading selected files", async () => {
  const original = await report();
  await savePendingSupportFiles(original);
  const first = await loadPendingSupportFiles();
  expect(first).toEqual(original);
  await savePendingSupportFiles(first!);
  new Uint8Array(original.files[0].bytes)[0] = 0;
  expect(await loadPendingSupportFiles()).toEqual(first);
  await clearPendingSupportFiles(first!.submission.submission_key);
  expect(await loadPendingSupportFiles()).toBeUndefined();
});

test("another tab cannot overwrite or delete a pending report", async () => {
  const first = await report();
  await savePendingSupportFiles(first);
  const second = await report();
  await expect(savePendingSupportFiles(second)).rejects.toThrow("different attachment report");
  await expect(clearPendingSupportFiles(second.submission.submission_key)).rejects.toThrow("Another report");
  expect(await loadPendingSupportFiles()).toEqual(first);
  const changed = structuredClone(first); changed.submission.body = "Changed after review";
  await expect(savePendingSupportFiles(changed)).rejects.toThrow("different attachment report");
  expect(await loadPendingSupportFiles()).toEqual(first);
});

test("changed bytes and unsafe metadata cannot become a safe retry copy", async () => {
  const original = await report();
  for (const change of ["hash", "type", "path", "count", "size", "empty"]) {
    const changed = structuredClone(original);
    if (change === "hash") new Uint8Array(changed.files[0].bytes)[0] = 0;
    if (change === "type") changed.files[0].metadata.media_type = "text/html";
    if (change === "path") changed.files[0].metadata.file_name = "../secret";
    if (change === "count") changed.files = Array(5).fill(changed.files[0]);
    if (change === "size") changed.files[0].metadata.size_bytes = 6 * 1024 * 1024;
    if (change === "empty") { changed.files[0].metadata.size_bytes = 0; changed.files[0].bytes = new ArrayBuffer(0); }
    await expect(savePendingSupportFiles(changed)).rejects.toThrow();
  }
  expect(await loadPendingSupportFiles()).toBeUndefined();
  await expect(validateSupportFiles([])).rejects.toThrow("one to four");
});

test("a stalled file reader is aborted without creating a pending report", async () => {
  vi.useFakeTimers();
  const abort = vi.fn();
  vi.stubGlobal("FileReader", class { abort = abort; readAsArrayBuffer() { /* intentionally stalled fixture */ } });
  const result = expect(prepareSupportFiles([new File(["one"],"fictional.txt",{type:"text/plain"})])).rejects.toThrow("could not be read");
  await vi.advanceTimersByTimeAsync(10_000);
  await result;
  expect(abort).toHaveBeenCalledTimes(1);
  vi.useRealTimers();
  expect(await loadPendingSupportFiles()).toBeUndefined();
});
