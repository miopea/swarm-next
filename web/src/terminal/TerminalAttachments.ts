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
