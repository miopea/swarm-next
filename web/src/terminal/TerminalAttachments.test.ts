import { afterEach, expect, test, vi } from "vitest";

import { clipboardImage, configureTerminalImageLimit, dragCarriesFiles, maxTerminalImageBytes, supportedImageSummary, terminalAttachmentPaste, terminalTextPaste, transferredImage, uploadTerminalImage } from "./TerminalAttachments";

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
/**
 * A drop and a paste do not fill the same fields. A paste populates `items`; a
 * drop is only reliable through `files`, and some sources leave `type` empty.
 * The fixture models both so a change cannot pass by satisfying one of them.
 */
function transfer(
  items: { kind: string; type: string; name?: string }[],
  options: { asDrop?: boolean } = {},
): DataTransfer {
  const asFiles = items
    .filter((item) => item.kind === "file")
    .map((item) => new File([""], item.name ?? "f", { type: item.type }));
  return {
    files: options.asDrop === false ? [] : asFiles,
    items: items.map((item) => ({
      ...item,
      getAsFile: () => new File([""], item.name ?? "f", { type: item.type }),
    })),
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

/**
 * The operator, after drop was wired: "I tried to drag and drop a gif over and
 * it shows like I can now but it never comes in."
 *
 * The drop target appeared, the file was accepted, and nothing happened.
 * `transferredImage` read only `items`, which a drop is not obliged to fill.
 */
test("a dropped GIF is found through files, not only items", () => {
  const dropped = {
    files: [new File([""], "party.gif", { type: "image/gif" })],
    items: [],
  } as unknown as DataTransfer;

  expect(transferredImage(dropped).kind).toBe("image");
});

/**
 * Some sources hand over a file with no media type at all. The name is then the
 * only evidence, and re-typing from it is safe because the server checks the
 * magic bytes and rejects a mislabel.
 */
test("a dropped file with no media type is typed from its name", () => {
  const dropped = {
    files: [new File([""], "party.gif", { type: "" })],
    items: [],
  } as unknown as DataTransfer;

  const result = transferredImage(dropped);

  expect(result.kind).toBe("image");
  expect(result.kind === "image" && result.file.type).toBe("image/gif");
});

test("a dropped file that is not an image is still refused by name", () => {
  const dropped = {
    files: [new File([""], "clip.mp4", { type: "video/mp4" })],
    items: [],
  } as unknown as DataTransfer;

  expect(transferredImage(dropped)).toEqual({ kind: "unsupported", description: "video/mp4" });
});

/**
 * The second miss on this feature, and the one that actually stopped it.
 *
 * A browser withholds item data while a drag is in progress — only `types` is
 * readable — so judging by `items` meant dragover sometimes declined to call
 * preventDefault. Preventing the default is what makes an element a drop
 * target, so the element was not one, the browser opened the file itself, and
 * the drop handler never ran.
 */
test("a drag carrying files is recognised from types alone", () => {
  const midDrag = { types: ["Files"], items: [] } as unknown as DataTransfer;

  expect(dragCarriesFiles(midDrag)).toBe(true);
});

test("a drag carrying only text is not a drop target", () => {
  const text = { types: ["text/plain"], items: [] } as unknown as DataTransfer;

  expect(dragCarriesFiles(text)).toBe(false);
});

/** Still recognised where the browser does populate items during the drag. */
test("a drag is recognised from items when types is unhelpful", () => {
  const legacy = {
    types: [],
    items: [{ kind: "file", type: "" }],
  } as unknown as DataTransfer;

  expect(dragCarriesFiles(legacy)).toBe(true);
});

/**
 * "The gif is nearly 16 megs. Do we have a limit?" We do — 8 MiB — and the
 * server enforces it with a transport body limit, which rejects the upload
 * before any code that could explain it. The operator got silence.
 */
test("an image over the limit is refused with its size and the limit", () => {
  configureTerminalImageLimit(8 * 1024 * 1024);
  const big = new File([new Uint8Array(1)], "party.gif", { type: "image/gif" });
  Object.defineProperty(big, "size", { value: 16 * 1024 * 1024 });
  const dropped = { types: ["Files"], files: [big], items: [] } as unknown as DataTransfer;

  const result = transferredImage(dropped);

  expect(result.kind).toBe("too-large");
  expect(result.kind === "too-large" && result.description).toContain("16.0 MB");
  expect(result.kind === "too-large" && result.description).toContain("8.0 MB");
});

test("an image inside the limit is still accepted", () => {
  configureTerminalImageLimit(8 * 1024 * 1024);
  const fine = new File([new Uint8Array(1)], "party.gif", { type: "image/gif" });
  Object.defineProperty(fine, "size", { value: 4 * 1024 * 1024 });
  const dropped = { types: ["Files"], files: [fine], items: [] } as unknown as DataTransfer;

  expect(transferredImage(dropped).kind).toBe("image");
});

/**
 * "I cannot shrink it, we need a reasonable limit." The limit moved to 32 MiB,
 * and the browser now takes the number from the Hive rather than holding its
 * own copy — a copy is what made the original failure silent, with the browser
 * believing one number and the route enforcing another.
 */
test("the limit comes from the Hive, not from a copy of it", () => {
  configureTerminalImageLimit(32 * 1024 * 1024);
  const recording = new File([new Uint8Array(1)], "bug.gif", { type: "image/gif" });
  Object.defineProperty(recording, "size", { value: 16 * 1024 * 1024 });
  const dropped = { types: ["Files"], files: [recording], items: [] } as unknown as DataTransfer;

  expect(maxTerminalImageBytes()).toBe(32 * 1024 * 1024);
  expect(transferredImage(dropped).kind).toBe("image");
});

test("an unusable limit is ignored rather than believed", () => {
  configureTerminalImageLimit(32 * 1024 * 1024);
  configureTerminalImageLimit(0);
  configureTerminalImageLimit(Number.NaN);

  expect(maxTerminalImageBytes()).toBe(32 * 1024 * 1024);
});
