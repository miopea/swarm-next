import type { SupportFile, SupportFileReport } from "../api/support";

const DATABASE = "swarm.support.files.v1";
const STORE = "reviewed";
const KEY = "pending";
export const SUPPORT_FILE_LIMIT = 5 * 1024 * 1024;
export const SUPPORT_FILES_LIMIT = 12 * 1024 * 1024;
const TYPES = ["image/png", "image/jpeg", "image/webp", "text/plain"];
const UUID = /^(?!00000000-0000-0000-0000-000000000000$)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const UNREADABLE = "Saved attachment report is unavailable. Check delivery status before creating a replacement.";
const isBuffer = (value: unknown): value is ArrayBuffer => Object.prototype.toString.call(value) === "[object ArrayBuffer]";

function checkMetadata(metadata: SupportFile["metadata"]) {
  if (!metadata || Object.keys(metadata).sort().join() !== "file_name,id,media_type,sha256,size_bytes"
    || !UUID.test(metadata.id) || typeof metadata.file_name !== "string" || !metadata.file_name.length || metadata.file_name.length > 180
    || /[\u0000-\u001f\u007f-\u009f/\\]/.test(metadata.file_name) || !TYPES.includes(metadata.media_type)
    || !Number.isSafeInteger(metadata.size_bytes) || metadata.size_bytes <= 0 || metadata.size_bytes > SUPPORT_FILE_LIMIT
    || !/^[0-9a-f]{64}$/.test(metadata.sha256)) throw new Error("Use nonempty PNG, JPEG, WebP or plain text files, up to 5 MiB each.");
}

async function digest(bytes: ArrayBuffer) {
  return [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((value) => value.toString(16).padStart(2, "0")).join("");
}

function readFileBytes(file: File): Promise<ArrayBuffer> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    let done = false;
    const fail = () => {
      if (done) return;
      done = true; clearTimeout(timer); reader.abort();
      reject(new Error("A selected file could not be read. Nothing was sent; select it again."));
    };
    const timer = setTimeout(fail, 10_000);
    reader.onerror = fail; reader.onabort = fail;
    reader.onload = () => {
      if (done) return;
      if (!isBuffer(reader.result)) { fail(); return; }
      done = true; clearTimeout(timer); resolve(reader.result);
    };
    try { reader.readAsArrayBuffer(file); } catch { fail(); }
  });
}

export async function validateSupportFiles(files: SupportFile[]): Promise<void> {
  if (!Array.isArray(files) || files.length < 1 || files.length > 4) throw new Error("Choose one to four attachments.");
  const ids = new Set<string>();
  let total = 0;
  for (const file of files) {
    checkMetadata(file?.metadata);
    if (!isBuffer(file.bytes) || file.bytes.byteLength !== file.metadata.size_bytes || ids.has(file.metadata.id)) throw new Error(UNREADABLE);
    ids.add(file.metadata.id); total += file.bytes.byteLength;
    if (total > SUPPORT_FILES_LIMIT) throw new Error("Attachments must total 12 MiB or less.");
    if (await digest(file.bytes) !== file.metadata.sha256) throw new Error(UNREADABLE);
    if (file.metadata.media_type === "text/plain") {
      let text: string;
      try { text = new TextDecoder("utf-8", { fatal: true }).decode(file.bytes); } catch { throw new Error("Text attachments must use UTF-8."); }
      if (text.includes("\0")) throw new Error("Text attachments cannot contain NUL characters.");
    }
  }
}

/** Freeze selected bytes before review; original paths/files are never reread on retry. */
export async function prepareSupportFiles(selected: File[]): Promise<SupportFile[]> {
  if (!selected.length || selected.length > 4) throw new Error("Choose one to four attachments.");
  if (selected.reduce((total, file) => total + file.size, 0) > SUPPORT_FILES_LIMIT) throw new Error("Attachments must total 12 MiB or less.");
  const files: SupportFile[] = [];
  for (const file of selected) {
    if (!file.size || file.size > SUPPORT_FILE_LIMIT || !TYPES.includes(file.type)) throw new Error("Use nonempty PNG, JPEG, WebP or plain text files, up to 5 MiB each.");
    const bytes = await readFileBytes(file);
    files.push({ metadata: { id: crypto.randomUUID(), file_name: file.name, media_type: file.type, size_bytes: bytes.byteLength, sha256: await digest(bytes) }, bytes });
  }
  await validateSupportFiles(files);
  return files;
}

async function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1);
    let done = false;
    const fail = () => { if (!done) { done = true; clearTimeout(timer); reject(new Error(UNREADABLE)); } };
    const timer = setTimeout(fail, 5000);
    request.onupgradeneeded = () => { request.result.createObjectStore(STORE); };
    request.onerror = fail; request.onblocked = fail;
    request.onsuccess = () => {
      const db = request.result;
      if (done) { db.close(); return; }
      done = true; clearTimeout(timer); db.onversionchange = () => db.close(); resolve(db);
    };
  });
}

async function transaction<T>(mode: IDBTransactionMode, work: (store: IDBObjectStore, set: (result: T) => void, fail: (message: string) => void) => void): Promise<T> {
  const db = await open();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, mode);
    let value: T; let failure = UNREADABLE;
    const timer = setTimeout(() => { try { tx.abort(); } catch { /* already settled */ } }, 5000);
    const finish = () => { clearTimeout(timer); db.close(); };
    tx.oncomplete = () => { finish(); resolve(value); };
    tx.onabort = () => { finish(); reject(new Error(failure)); };
    tx.onerror = () => { failure = "Browser could not retain these files safely. Nothing new was sent; keep this report and retry."; };
    try { work(tx.objectStore(STORE), (result) => { value = result; }, (message) => { failure = message; tx.abort(); }); }
    catch { tx.abort(); }
  });
}

function same(a: SupportFileReport, b: SupportFileReport): boolean {
  return Boolean(a) && JSON.stringify(a.submission) === JSON.stringify(b.submission)
    && Array.isArray(a.files) && a.files.length === b.files.length && a.files.every((file, index) => {
      const other = b.files[index];
      if (!file || JSON.stringify(file.metadata) !== JSON.stringify(other.metadata)
        || !isBuffer(file.bytes) || file.bytes.byteLength !== other.bytes.byteLength) return false;
      const right = new Uint8Array(other.bytes);
      return new Uint8Array(file.bytes).every((value, offset) => value === right[offset]);
    });
}

/** One atomic browser record, bounded to one report. No background sender or timers. */
export async function savePendingSupportFiles(report: SupportFileReport): Promise<void> {
  await validateSupportFiles(report.files);
  if (!UUID.test(report.submission.submission_key) || new TextEncoder().encode(JSON.stringify(report.submission)).length > 128 * 1024) throw new Error(UNREADABLE);
  await transaction<void>("readwrite", (store, set, fail) => {
    const get = store.get(KEY);
    get.onsuccess = () => {
      if (get.result && !same(get.result as SupportFileReport, report)) { fail("A different attachment report is still awaiting confirmation. Recover it before sending another."); return; }
      store.put(report, KEY); set(undefined);
    };
  });
}

export async function loadPendingSupportFiles(): Promise<SupportFileReport | undefined> {
  const report = await transaction<SupportFileReport | undefined>("readonly", (store, set) => { const get = store.get(KEY); get.onsuccess = () => set(get.result as SupportFileReport | undefined); });
  if (!report) return;
  if (!report.submission || !UUID.test(report.submission.submission_key)
    || !["feedback", "bug_report", "feature_request"].includes(report.submission.kind)
    || typeof report.submission.email !== "string" || !(report.submission.name === null || typeof report.submission.name === "string")
    || typeof report.submission.subject !== "string" || typeof report.submission.body !== "string"
    || Object.keys(report.submission).some((key) => !["submission_key", "kind", "email", "name", "subject", "body"].includes(key))) throw new Error(UNREADABLE);
  await validateSupportFiles(report.files);
  return report;
}

/** Only a matching confirmed Hive save allows removal; never delete another tab's report. */
export async function clearPendingSupportFiles(key: string): Promise<void> {
  await transaction<void>("readwrite", (store, set, fail) => {
    const get = store.get(KEY);
    get.onsuccess = () => {
      if (get.result && (get.result as SupportFileReport).submission?.submission_key !== key) { fail("Another report is pending; its files were retained."); return; }
      store.delete(KEY); set(undefined);
    };
  });
}
