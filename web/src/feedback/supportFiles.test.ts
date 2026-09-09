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
  for (const change of ["hash", "type", "path", "count", "size"]) {
    const changed = structuredClone(original);
    if (change === "hash") new Uint8Array(changed.files[0].bytes)[0] = 0;
    if (change === "type") changed.files[0].metadata.media_type = "text/html";
    if (change === "path") changed.files[0].metadata.file_name = "../secret";
    if (change === "count") changed.files = Array(5).fill(changed.files[0]);
    if (change === "size") changed.files[0].metadata.size_bytes = 6 * 1024 * 1024;
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
