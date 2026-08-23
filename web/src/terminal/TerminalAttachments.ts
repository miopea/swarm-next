import { authenticatedFetch, recoverTransientRuntime } from "../api";

export const SUPPORTED_TERMINAL_IMAGE_TYPES = new Set([
  "image/png",
  "image/jpeg",
  "image/webp",
  "image/gif",
]);

export function clipboardImage(clipboard: DataTransfer): File | undefined {
  for (const item of clipboard.items) {
    if (item.kind !== "file" || !SUPPORTED_TERMINAL_IMAGE_TYPES.has(item.type)) continue;
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
export type TransferredImage =
  | { kind: "image"; file: File }
  | { kind: "unsupported"; description: string }
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

/** Extensions to fall back on when a transfer carries no media type. */
const IMAGE_EXTENSIONS = new Map([
  ["png", "image/png"],
  ["jpg", "image/jpeg"],
  ["jpeg", "image/jpeg"],
  ["webp", "image/webp"],
  ["gif", "image/gif"],
]);

/** The media type of a dropped file, from its own type or from its name. */
function imageTypeOf(file: File): string | undefined {
  if (SUPPORTED_TERMINAL_IMAGE_TYPES.has(file.type)) return file.type;
  const extension = file.name.split(".").pop()?.toLowerCase();
  return extension ? IMAGE_EXTENSIONS.get(extension) : undefined;
}

export function transferredImage(transfer: DataTransfer): TransferredImage {
  // `files` first, because this is the only source a drop can be relied on to
  // populate. Reading `items` alone worked for paste and silently found nothing
  // on drop — the drop target appeared, the file was accepted, and nothing
  // happened, which is exactly what a missing feature looks like.
  // Defensive: a text paste carries no files, and not every source populates
  // the field at all. Throwing here would take the text paste down with it.
  for (const file of Array.from(transfer.files ?? [])) {
    const type = imageTypeOf(file);
    // A file whose type the browser did not fill in still has a name. Re-typing
    // it here is safe: the server checks the magic bytes and rejects a mislabel.
    if (type) return { kind: "image", file: type === file.type ? file : new File([file], file.name, { type }) };
  }
  const image = clipboardImage(transfer);
  if (image) return { kind: "image", file: image };

  const described = Array.from(transfer.files ?? []).map((file) => file.type || file.name);
  if (described.length > 0) {
    return { kind: "unsupported", description: [...new Set(described)].join(", ") };
  }
  const items = Array.from(transfer.items ?? []).filter((item) => item.kind === "file");
  if (items.length === 0) return { kind: "none" };
  const types = [...new Set(items.map((item) => item.type || "an unknown type"))];
  return { kind: "unsupported", description: types.join(", ") };
}

/** The formats a terminal accepts, for saying so. */
export function supportedImageSummary(): string {
  return [...SUPPORTED_TERMINAL_IMAGE_TYPES]
    .map((type) => type.replace("image/", "").toUpperCase())
    .join(", ");
}

/**
 * Uploads one image and returns the path to paste.
 *
 * Retried through the same transient-failure path as the rest of the runtime:
 * an upload that fails because the API is restarting succeeds on a second
 * attempt, and the operator should not have to be the one making it.
 */
export async function uploadTerminalImage(
  operatorToken: string,
  sessionId: string,
  image: File,
): Promise<string> {
  const response = await recoverTransientRuntime(() => authenticatedFetch(
    operatorToken,
    `/api/v1/terminal/sessions/${encodeURIComponent(sessionId)}/attachments`,
    { method: "POST", headers: { "Content-Type": image.type }, body: image },
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
