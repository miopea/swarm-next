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

export function transferredImage(transfer: DataTransfer): TransferredImage {
  const image = clipboardImage(transfer);
  if (image) return { kind: "image", file: image };
  const files = [...transfer.items].filter((item) => item.kind === "file");
  if (files.length === 0) return { kind: "none" };
  const types = [...new Set(files.map((item) => item.type || "an unknown type"))];
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
