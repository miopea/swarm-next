import { authenticatedFetch, recoverTransientRuntime } from "../api";

/** The Open XML media type for a modern Excel workbook. */
export const XLSX_TYPE =
  "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
/** The media type for a legacy Excel workbook. */
export const XLS_TYPE = "application/vnd.ms-excel";

/**
 * Everything a terminal will take, and it is no longer only images.
 *
 * Kept in step with validated_extension in crates/swarm-api/src/attachments.rs,
 * which re-checks every one of these against the file's own bytes. Adding a type
 * here alone gets it rejected by the server, which is the safe direction.
 */
export const SUPPORTED_TERMINAL_ATTACHMENT_TYPES = new Set([
  "image/png",
  "image/jpeg",
  "image/webp",
  "image/gif",
  "text/csv",
  XLSX_TYPE,
  XLS_TYPE,
]);

export function clipboardAttachment(clipboard: DataTransfer): File | undefined {
  for (const item of clipboard.items) {
    if (item.kind !== "file" || !SUPPORTED_TERMINAL_ATTACHMENT_TYPES.has(item.type)) continue;
    const file = item.getAsFile();
    if (file) return file;
  }
  return undefined;
}

/**
 * What a drop or paste carried, told apart so the operator can be told why.
 *
 * A file that is not a supported image used to be indistinguishable from no
 * file at all: both did nothing, silently. "Seems that copy and paste doesn't
 * work for .gif either" is what that looks like from the outside — and it is
 * also what it looks like when the clipboard never held a file in the first
 * place, which is what a browser gives you when you copy an image out of a web
 * page rather than a file out of a file manager.
 */
/**
 * The largest image a terminal will take, matching MAX_ATTACHMENT_BYTES in
 * crates/swarm-api/src/attachments.rs.
 *
 * Checked here as well as there because the server enforces it with a transport
 * body limit, which rejects the upload before any code that could explain why.
 * The operator dropped a 16 MB GIF and got silence.
 */
const DEFAULT_TERMINAL_IMAGE_BYTES = 32 * 1024 * 1024;
let terminalImageLimit = DEFAULT_TERMINAL_IMAGE_BYTES;

/**
 * Adopts the server's real limit, so the refusal quotes a number that is true.
 *
 * A copied constant is what made the original failure silent: the browser
 * believed one number, the route enforced another, and an oversized upload died
 * in a transport layer with nothing to say. The default is only what to believe
 * before the Hive has been asked.
 */
export function configureTerminalImageLimit(bytes: number): void {
  if (Number.isFinite(bytes) && bytes > 0) terminalImageLimit = bytes;
}

export function maxTerminalAttachmentBytes(): number {
  return terminalImageLimit;
}

/** Human sizes, for saying what was too big and by how much. */
export function describeBytes(bytes: number): string {
  const mib = bytes / (1024 * 1024);
  return mib >= 1 ? `${mib.toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export type TransferredAttachment =
  | { kind: "file"; file: File }
  | { kind: "unsupported"; description: string }
  | { kind: "too-large"; description: string }
  | { kind: "none" };

/**
 * Whether a drag is carrying files, judged during the drag itself.
 *
 * `dataTransfer.types` is the only part of a transfer a browser exposes while a
 * drag is still in progress; item data is withheld until the drop, and
 * `items[].type` is routinely empty until then. Deciding with `items` meant the
 * dragover handler sometimes declined to call preventDefault — and preventing
 * the default is precisely what makes an element a drop target. The element was
 * not one, so the browser opened the file itself and the drop handler was never
 * reached at all.
 */
export function dragCarriesFiles(transfer: DataTransfer): boolean {
  if (Array.from(transfer.types ?? []).includes("Files")) return true;
  return Array.from(transfer.items ?? []).some((item) => item.kind === "file");
}

/**
 * Extensions to fall back on when a transfer carries no media type.
 *
 * This matters more for spreadsheets than it ever did for images. A CSV dragged
 * out of a file manager frequently arrives with an empty `type`, and Excel files
 * are routinely handed over as application/octet-stream, so without this a drop
 * that plainly said ".csv" in its own name would be refused as unsupported.
 */
const ATTACHMENT_EXTENSIONS = new Map([
  ["png", "image/png"],
  ["jpg", "image/jpeg"],
  ["jpeg", "image/jpeg"],
  ["webp", "image/webp"],
  ["gif", "image/gif"],
  ["csv", "text/csv"],
  ["xlsx", XLSX_TYPE],
  ["xls", XLS_TYPE],
]);

/** The media type of a dropped file, from its own type or from its name. */
function attachmentTypeOf(file: File): string | undefined {
  if (SUPPORTED_TERMINAL_ATTACHMENT_TYPES.has(file.type)) return file.type;
  const extension = file.name.split(".").pop()?.toLowerCase();
  return extension ? ATTACHMENT_EXTENSIONS.get(extension) : undefined;
}

export function transferredAttachment(transfer: DataTransfer): TransferredAttachment {
  // `files` first, because this is the only source a drop can be relied on to
  // populate. Reading `items` alone worked for paste and silently found nothing
  // on drop — the drop target appeared, the file was accepted, and nothing
  // happened, which is exactly what a missing feature looks like.
  // Defensive: a text paste carries no files, and not every source populates
  // the field at all. Throwing here would take the text paste down with it.
  for (const file of Array.from(transfer.files ?? [])) {
    const type = attachmentTypeOf(file);
    // A file whose type the browser did not fill in still has a name. Re-typing
    // it here is safe: the server checks the magic bytes and rejects a mislabel.
    if (!type) continue;
    if (file.size > maxTerminalAttachmentBytes()) {
      return {
        kind: "too-large",
        description: `${file.name} is ${describeBytes(file.size)}; the limit is ${describeBytes(maxTerminalAttachmentBytes())}`,
      };
    }
    return { kind: "file", file: type === file.type ? file : new File([file], file.name, { type }) };
  }
  const image = clipboardAttachment(transfer);
  if (image) {
    return image.size > maxTerminalAttachmentBytes()
      ? {
          kind: "too-large",
          description: `that image is ${describeBytes(image.size)}; the limit is ${describeBytes(maxTerminalAttachmentBytes())}`,
        }
      : { kind: "file", file: image };
  }

  const described = Array.from(transfer.files ?? []).map((file) => file.type || file.name);
  if (described.length > 0) {
    return { kind: "unsupported", description: [...new Set(described)].join(", ") };
  }
  const items = Array.from(transfer.items ?? []).filter((item) => item.kind === "file");
  if (items.length === 0) return { kind: "none" };
  const types = [...new Set(items.map((item) => item.type || "an unknown type"))];
  return { kind: "unsupported", description: types.join(", ") };
}

/**
 * The formats a terminal accepts, for saying so.
 *
 * Built from the extension map rather than the media types, because the media
 * type of an xlsx is 66 characters of Open XML boilerplate and nobody being told
 * why their drop failed wants to read it.
 */
export function supportedAttachmentSummary(): string {
  return [...new Set(ATTACHMENT_EXTENSIONS.keys())]
    .map((extension) => extension.toUpperCase())
    .join(", ");
}

/**
 * Uploads one attachment and returns the path to paste.
 *
 * Retried through the same transient-failure path as the rest of the runtime:
 * an upload that fails because the API is restarting succeeds on a second
 * attempt, and the operator should not have to be the one making it.
 */
export async function uploadTerminalAttachment(
  operatorToken: string,
  sessionId: string,
  file: File,
): Promise<string> {
  const response = await recoverTransientRuntime(() => authenticatedFetch(
    operatorToken,
    `/api/v1/terminal/sessions/${encodeURIComponent(sessionId)}/attachments`,
    { method: "POST", headers: { "Content-Type": file.type }, body: file },
  ));
  return ((await response.json()) as { path: string }).path;
}

export function terminalAttachmentPaste(path: string): string {
  return `\u001b[200~${path}\u001b[201~ `;
}

export function terminalTextPaste(text: string): string {
  const normalized = text.replace(/\r\n?/g, "\n");
  return `\u001b[200~${normalized}\u001b[201~`;
}
