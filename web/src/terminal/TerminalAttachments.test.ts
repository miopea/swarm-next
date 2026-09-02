import { afterEach, expect, test, vi } from "vitest";

import { TERMINAL_ATTACHMENT_ACCEPT, chosenAttachment, clipboardAttachment, configureTerminalImageLimit, dragCarriesFiles, maxTerminalAttachmentBytes, terminalAttachmentPaste, terminalTextPaste, transferredAttachment, uploadTerminalAttachment } from "./TerminalAttachments";

afterEach(() => vi.unstubAllGlobals());

test("selects a supported clipboard image without consuming ordinary text", () => {
  const image = new File([new Uint8Array([1, 2, 3])], "capture.png", { type: "image/png" });
  const imageTransfer = {
    items: [{ kind: "file", type: "image/png", getAsFile: () => image }],
  } as unknown as DataTransfer;
  const textTransfer = {
    items: [{ kind: "string", type: "text/plain", getAsFile: () => null }],
  } as unknown as DataTransfer;

  expect(clipboardAttachment(imageTransfer)).toBe(image);
  expect(clipboardAttachment(textTransfer)).toBeUndefined();
});

test("inserts a private image path without submitting the terminal prompt", () => {
  const input = terminalAttachmentPaste("/state/attachments/capture.png");
  expect(input).toBe("\u001b[200~/state/attachments/capture.png\u001b[201~ ");
  expect(input).not.toContain("\r");
});

/**
 * The defect: a docx or an mp4 uploaded fine and left ONLY a space in the
 * composer, because the provider swallows a bare pasted path it cannot attach
 * and the trailing space is all that survives. A space reads as nothing having
 * happened, while the file is in fact attached.
 *
 * ⚠️ THE FIRST FIX ADDED AN `@` PREFIX AND SHIPPED, AND IT DID NOT WORK. This
 * test asserted `ESC[200~@<path>ESC[201~ ` and passed, because it checked the
 * bytes we send rather than what the provider does with them. The operator
 * tested the shipped build with an mp4, a zip and a spreadsheet: "just one extra
 * space was shown" — the identical symptom.
 *
 * The space is OUTSIDE the markers, so one space and nothing else proves the
 * provider discarded everything BETWEEN them. The prefix never mattered; the
 * BRACKETED PASTE did. A non-image reference is now typed, through the ordinary
 * input path rather than the paste handler.
 *
 * This test still cannot prove the fix works — it asserts what we send, and only
 * a drop on a live terminal shows what the provider does. 01a06238 holds that.
 */
test("types a non-image reference instead of pasting it", () => {
  for (const path of [
    "/state/attachments/report.docx",
    "/state/attachments/clip.mp4",
    "/state/attachments/archive.zip",
    "/state/attachments/opaque.bin",
  ]) {
    const input = terminalAttachmentPaste(path);
    expect(input).toBe(`@${path} `);
    // NOT a bracketed paste: that wrapper is the thing the provider ate.
    expect(input).not.toContain("\u001b[200~");
    expect(input).not.toContain("\r");
    // The whole point: something other than whitespace reaches the composer.
    expect(input.trim()).not.toBe("");
  }
});

/**
 * Images are untouched. They render as a chip today, which is the one case that
 * was never broken — and changing it to fix a different case is how a working
 * path becomes collateral.
 */
test("an image keeps the bracketed paste it already works with", () => {
  expect(terminalAttachmentPaste("/state/attachments/capture.jpg"))
    .toBe("\u001b[200~/state/attachments/capture.jpg\u001b[201~ ");
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

  return expect(uploadTerminalAttachment("token", "session-1", image))
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
  const result = transferredAttachment(transfer([{ kind: "file", type: "image/gif" }]));

  expect(result.kind).toBe("file");
});

/**
 * This test used to assert the opposite, and the change is deliberate.
 *
 * It was written when the terminal took images only, where refusing an mp4 BY
 * NAME was the fix — the bug it guarded was refusing silently. The operator has
 * since asked for most file types, so an mp4 is now simply taken, and its media
 * type is preserved rather than flattened to octet-stream.
 */
test("a video is taken now, with its own media type", () => {
  const result = transferredAttachment(transfer([{ kind: "file", type: "video/mp4" }]));

  expect(result.kind).toBe("file");
  if (result.kind === "file") expect(result.file.type).toBe("video/mp4");
});

/**
 * Copying an image out of a web page gives you no file at all, only markup and
 * a URL. That has to stay distinguishable from dropping something unsupported,
 * because the remedy is different and this path still has to paste text.
 */
test("a clipboard carrying no file at all is not an unsupported file", () => {
  const result = transferredAttachment(transfer([{ kind: "string", type: "text/html" }]));

  expect(result).toEqual({ kind: "none" });
});

/**
 * What the extension map is FOR, now that it no longer gates anything.
 *
 * A browser that leaves `type` empty would otherwise get octet-stream, and the
 * server would store a PNG as .bin. The map exists to give a better answer than
 * the fallback, not to decide what is allowed.
 */
test("a known extension beats the octet-stream fallback when the browser says nothing", () => {
  const png = transferredAttachment(transfer([{ kind: "file", type: "", name: "shot.png" }]));

  expect(png.kind).toBe("file");
  if (png.kind === "file") expect(png.file.type).toBe("image/png");
});

/**
 * The operator, after drop was wired: "I tried to drag and drop a gif over and
 * it shows like I can now but it never comes in."
 *
 * The drop target appeared, the file was accepted, and nothing happened.
 * `transferredAttachment` read only `items`, which a drop is not obliged to fill.
 */
test("a dropped GIF is found through files, not only items", () => {
  const dropped = {
    files: [new File([""], "party.gif", { type: "image/gif" })],
    items: [],
  } as unknown as DataTransfer;

  expect(transferredAttachment(dropped).kind).toBe("file");
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

  const result = transferredAttachment(dropped);

  expect(result.kind).toBe("file");
  expect(result.kind === "file" && result.file.type).toBe("image/gif");
});

test("a dropped non-image is taken through the files path too", () => {
  const dropped = {
    files: [new File([""], "clip.mp4", { type: "video/mp4" })],
    items: [],
  } as unknown as DataTransfer;

  const result = transferredAttachment(dropped);

  expect(result.kind).toBe("file");
  if (result.kind === "file") expect(result.file.name).toBe("clip.mp4");
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

  const result = transferredAttachment(dropped);

  expect(result.kind).toBe("too-large");
  expect(result.kind === "too-large" && result.description).toContain("16.0 MB");
  expect(result.kind === "too-large" && result.description).toContain("8.0 MB");
});

test("an image inside the limit is still accepted", () => {
  configureTerminalImageLimit(8 * 1024 * 1024);
  const fine = new File([new Uint8Array(1)], "party.gif", { type: "image/gif" });
  Object.defineProperty(fine, "size", { value: 4 * 1024 * 1024 });
  const dropped = { types: ["Files"], files: [fine], items: [] } as unknown as DataTransfer;

  expect(transferredAttachment(dropped).kind).toBe("file");
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

  expect(maxTerminalAttachmentBytes()).toBe(32 * 1024 * 1024);
  expect(transferredAttachment(dropped).kind).toBe("file");
});

test("an unusable limit is ignored rather than believed", () => {
  configureTerminalImageLimit(32 * 1024 * 1024);
  configureTerminalImageLimit(0);
  configureTerminalImageLimit(Number.NaN);

  expect(maxTerminalAttachmentBytes()).toBe(32 * 1024 * 1024);
});

/**
 * The operator, 2026-08-26: "I am not able to drag/drop an csv or excel
 * document into the chat."
 *
 * The drop path was built for images and the allow-list said so, so a
 * spreadsheet was refused by the same code that refuses a random binary. These
 * assert the two shapes a spreadsheet actually arrives in — one that declares
 * its media type, and one that does not.
 */
test("a CSV is a file this terminal accepts", () => {
  const result = transferredAttachment(transfer([{ kind: "file", type: "text/csv", name: "budget.csv" }]));

  expect(result.kind).toBe("file");
});

/**
 * The case that matters most in practice. A CSV dragged out of a file manager
 * frequently arrives with an empty `type`, so without the extension fallback the
 * file the operator can plainly see is named .csv gets refused as unsupported.
 */
test("a CSV whose media type the browser did not fill in is still accepted", () => {
  const result = transferredAttachment(transfer([{ kind: "file", type: "", name: "export.csv" }]));

  expect(result.kind).toBe("file");
  if (result.kind === "file") expect(result.file.type).toBe("text/csv");
});

test("an Excel workbook is accepted by extension and by media type", () => {
  const byName = transferredAttachment(transfer([{ kind: "file", type: "", name: "q3.xlsx" }]));
  const byType = transferredAttachment(
    transfer([
      {
        kind: "file",
        type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        name: "q3.xlsx",
      },
    ]),
  );

  expect(byName.kind).toBe("file");
  expect(byType.kind).toBe("file");
});

/**
 * The ablation. Widening the allow-list must not widen it to everything — if
 * this passes as "file", the type gate has stopped gating.
 */
/**
 * The operator, after the CSV fix: "we should definitely be able to drag and
 * drop most file types."
 *
 * So the allow-list is gone. A type nobody enumerated is accepted and handed to
 * the server, which stores it opaquely. If this ever returns anything but
 * "file", the client has started refusing things again.
 */
test("a file type nobody enumerated is still accepted", () => {
  const zip = transferredAttachment(transfer([{ kind: "file", type: "application/zip", name: "a.zip" }]));
  const odd = transferredAttachment(transfer([{ kind: "file", type: "application/x-sqlite3", name: "hive.db" }]));

  expect(zip.kind).toBe("file");
  expect(odd.kind).toBe("file");
});

/**
 * A file with no media type and no recognised extension still uploads, typed as
 * octet-stream. Before this, it was dropped on the floor by `continue`.
 */
test("a file the browser could not type at all is sent as octet-stream", () => {
  const result = transferredAttachment(transfer([{ kind: "file", type: "", name: "LICENSE" }]));

  expect(result.kind).toBe("file");
  if (result.kind === "file") expect(result.file.type).toBe("application/octet-stream");
});

/**
 * THE TWO PATHS MUST AGREE, and they did not.
 *
 * A dropped file was size-checked and refused by name; a file picked from a
 * phone went straight to the upload, so an oversized one ran until the
 * server's transport limit killed it. Same product, same question, two
 * answers — and the operator met the second one as a video that seemed to
 * upload and then failed with nothing legible.
 */
test("a picked file is judged exactly as a dropped one is", () => {
  const oversized = new File([new Uint8Array(1)], "clip.mov", { type: "video/quicktime" });
  Object.defineProperty(oversized, "size", { value: maxTerminalAttachmentBytes() + 1 });

  const picked = chosenAttachment(oversized);
  expect(picked.kind).toBe("too-large");
  // Names the file and the limit, not a generic failure.
  expect(picked.kind === "too-large" && picked.description).toContain("clip.mov");
  expect(picked.kind === "too-large" && picked.description).toContain("the limit is");
});

test("video is not a special case: a small one is accepted like any other file", () => {
  // The settled policy is MOST FILE TYPES (v0.8.19). Video is caught by size,
  // which is what actually makes it unusable — not by a type rule nobody wrote.
  const small = new File([new Uint8Array([1, 2, 3])], "clip.mov", { type: "video/quicktime" });
  const picked = chosenAttachment(small);
  expect(picked.kind).toBe("file");
});

test("the picker asks for the families this product carries, and not video", () => {
  // `accept` has no negation, so the only way to leave video out is to name
  // what is in. This must keep the families the drop path was widened to take.
  expect(TERMINAL_ATTACHMENT_ACCEPT).toContain("image/*");
  expect(TERMINAL_ATTACHMENT_ACCEPT).toContain("text/*");
  expect(TERMINAL_ATTACHMENT_ACCEPT).toContain("application/*");
  expect(TERMINAL_ATTACHMENT_ACCEPT).not.toContain("video/");
  expect(TERMINAL_ATTACHMENT_ACCEPT).not.toContain("*/*");
});
