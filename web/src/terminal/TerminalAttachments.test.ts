import { afterEach, expect, test, vi } from "vitest";

import { clipboardImage, supportedImageSummary, terminalAttachmentPaste, terminalTextPaste, transferredImage, uploadTerminalImage } from "./TerminalAttachments";

afterEach(() => vi.unstubAllGlobals());

test("selects a supported clipboard image without consuming ordinary text", () => {
  const image = new File([new Uint8Array([1, 2, 3])], "capture.png", { type: "image/png" });
  const imageTransfer = {
    items: [{ kind: "file", type: "image/png", getAsFile: () => image }],
  } as unknown as DataTransfer;
  const textTransfer = {
    items: [{ kind: "string", type: "text/plain", getAsFile: () => null }],
  } as unknown as DataTransfer;

  expect(clipboardImage(imageTransfer)).toBe(image);
  expect(clipboardImage(textTransfer)).toBeUndefined();
});

test("inserts a private image path without submitting the terminal prompt", () => {
  const input = terminalAttachmentPaste("/state/attachments/capture.png");
  expect(input).toBe("\u001b[200~/state/attachments/capture.png\u001b[201~ ");
  expect(input).not.toContain("\r");
});

test("intercepts text as one bracketed terminal paste", () => {
  expect(terminalTextPaste("first\r\nsecond\rthird"))
    .toBe("\u001b[200~first\nsecond\nthird\u001b[201~");
});

test("carries an image through the API restarting underneath it", () => {
  // "Image could not be added" once, then the same image worked on a second
  // try. The operator was doing by hand what the rest of the runtime already
  // does for itself.
  const fetch = vi.fn()
    .mockResolvedValueOnce(new Response("", { status: 503 }))
    .mockResolvedValue(new Response(JSON.stringify({ path: "/state/attachments/capture.png" }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    }));
  vi.stubGlobal("fetch", fetch);
  const image = new File([new Uint8Array([1, 2, 3])], "capture.png", { type: "image/png" });

  return expect(uploadTerminalImage("token", "session-1", image))
    .resolves.toBe("/state/attachments/capture.png")
    .then(() => expect(fetch).toHaveBeenCalledTimes(2));
});

/**
 * "I tried to drag and drop an animated gif into the chat and it won't work.
 * Seems that copy and paste doesn't work for .gif either."
 *
 * GIF was in the accepted set the whole time, client and server. What was not
 * there was any way to tell the operator why nothing happened — a file that is
 * not a supported image did exactly what an empty clipboard did, which is
 * nothing at all.
 */
function transfer(items: { kind: string; type: string }[]): DataTransfer {
  return {
    items: items.map((item) => ({ ...item, getAsFile: () => new File([""], "f", { type: item.type }) })),
  } as unknown as DataTransfer;
}

test("a GIF is an image this terminal accepts", () => {
  const result = transferredImage(transfer([{ kind: "file", type: "image/gif" }]));

  expect(result.kind).toBe("image");
});

test("a file that is not a supported image is refused by name, not ignored", () => {
  const result = transferredImage(transfer([{ kind: "file", type: "video/mp4" }]));

  expect(result).toEqual({ kind: "unsupported", description: "video/mp4" });
});

/**
 * Copying an image out of a web page gives you no file at all, only markup and
 * a URL. That has to stay distinguishable from dropping something unsupported,
 * because the remedy is different and this path still has to paste text.
 */
test("a clipboard carrying no file at all is not an unsupported file", () => {
  const result = transferredImage(transfer([{ kind: "string", type: "text/html" }]));

  expect(result).toEqual({ kind: "none" });
});

test("names the formats it does accept, so the message can say", () => {
  expect(supportedImageSummary()).toContain("GIF");
  expect(supportedImageSummary()).toContain("PNG");
});
