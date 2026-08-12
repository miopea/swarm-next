import { expect, test } from "vitest";

import { clipboardImage, terminalAttachmentPaste, terminalTextPaste } from "./TerminalAttachments";

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
