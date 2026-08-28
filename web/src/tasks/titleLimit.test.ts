import { describe, expect, it } from "vitest";

import { TITLE_BYTE_LIMIT, clampTitleToBytes, titleByteLength, titleFits } from "./titleLimit";

describe("titleLimit", () => {
  it("counts what the server counts, not what maxLength counts", () => {
    // The exact mismatch behind the report. 240 typographic characters are
    // well inside a maxLength={240} field and well outside the server's limit.
    const typographic = "…".repeat(240);
    expect(typographic.length).toBe(240);
    expect(titleByteLength(typographic)).toBe(720);
    expect(titleFits(typographic)).toBe(false);
  });

  it("accepts an ordinary title unchanged", () => {
    const title = "Plates show on the rest card but not the active card";
    expect(titleFits(title)).toBe(true);
    expect(clampTitleToBytes(title)).toBe(title);
  });

  it("clamps a real email subject to something the server will take", () => {
    // A mail client's punctuation: curly quotes and an em dash, 3 bytes each.
    const subject = `${"Re: the worker’s state isn’t updating — see thread ".repeat(6)}end`;
    expect(titleByteLength(subject)).toBeGreaterThan(TITLE_BYTE_LIMIT);
    const clamped = clampTitleToBytes(subject);
    expect(titleByteLength(clamped)).toBeLessThanOrEqual(TITLE_BYTE_LIMIT);
    expect(titleFits(clamped)).toBe(true);
    expect(clamped.endsWith("…")).toBe(true);
  });

  it("never cuts a character in half", () => {
    // Every character is 3 bytes, so a naive byte slice at 240 lands inside one.
    const clamped = clampTitleToBytes("界".repeat(200));
    expect(titleByteLength(clamped)).toBeLessThanOrEqual(TITLE_BYTE_LIMIT);
    // A split code point decodes as U+FFFD; its absence is the assertion.
    expect(clamped).not.toContain("�");
    expect([...clamped].every((c) => c === "界" || c === "…")).toBe(true);
  });

  it("keeps an emoji whole rather than splitting its sequence", () => {
    const family = "👨‍👩‍👧‍👦";
    const clamped = clampTitleToBytes(family.repeat(20));
    expect(titleByteLength(clamped)).toBeLessThanOrEqual(TITLE_BYTE_LIMIT);
    // A split ZWJ sequence leaves a trailing joiner or a bare member.
    expect(clamped.replace(/…$/, "").endsWith("‍")).toBe(false);
  });

  it("says so visibly when it shortened something", () => {
    expect(clampTitleToBytes("a".repeat(300)).endsWith("…")).toBe(true);
    expect(clampTitleToBytes("a".repeat(10)).endsWith("…")).toBe(false);
  });

  it("produces a result the server accepts at the exact boundary", () => {
    const exact = "a".repeat(TITLE_BYTE_LIMIT);
    expect(titleFits(exact)).toBe(true);
    expect(clampTitleToBytes(exact)).toBe(exact);
    expect(titleFits(`${exact}a`)).toBe(false);
  });
});
