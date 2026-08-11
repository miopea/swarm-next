import { expect, test } from "vitest";

import { terminalTheme } from "./terminalTheme";

const readableAnsiKeys = [
  "red", "green", "yellow", "blue", "magenta", "cyan", "white", "brightBlack",
  "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
] as const;

test.each(["light", "dark"] as const)("the %s terminal palette keeps ANSI output readable", (mode) => {
  const theme = terminalTheme(mode);
  expect(theme.background).toBeDefined();
  expect(theme.foreground).toBeDefined();
  expect(theme.cursor).toBeDefined();
  expect(theme.cursorAccent).toBeDefined();
  expect(theme.selectionBackground).toBeDefined();
  expect(theme.selectionForeground).toBeDefined();
  expect(theme.red).not.toBe(theme.green);

  for (const key of readableAnsiKeys) {
    expect(contrast(theme[key]!, theme.background!), `${key} contrast`).toBeGreaterThanOrEqual(4.5);
  }
});

test("light and dark shells receive distinct near-black terminal treatments", () => {
  const light = terminalTheme("light");
  const dark = terminalTheme("dark");
  expect(light.background).not.toBe(dark.background);
  expect(light.cursor).not.toBe(dark.cursor);
});

function contrast(foreground: string, background: string): number {
  const lighter = Math.max(luminance(foreground), luminance(background));
  const darker = Math.min(luminance(foreground), luminance(background));
  return (lighter + 0.05) / (darker + 0.05);
}

function luminance(hex: string): number {
  const channels = hex.slice(1, 7).match(/.{2}/g)!.map((channel) => Number.parseInt(channel, 16) / 255);
  const [red, green, blue] = channels.map((channel) => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}
