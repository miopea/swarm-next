import type { ITheme } from "@xterm/xterm";

import type { ColorTheme } from "./theme";

const ANSI_COLORS = {
  black: "#17201d",
  red: "#d98b86",
  green: "#9eb68b",
  yellow: "#d9ad58",
  blue: "#87afc4",
  magenta: "#b7a0c8",
  cyan: "#82b8ae",
  white: "#d8d4c7",
  brightBlack: "#78867c",
  brightRed: "#f0a09a",
  brightGreen: "#bad19f",
  brightYellow: "#f0ca78",
  brightBlue: "#a8cee0",
  brightMagenta: "#d4bde2",
  brightCyan: "#a2d4ca",
  brightWhite: "#fff9e8",
} satisfies Partial<ITheme>;

export function terminalTheme(colorTheme: ColorTheme): ITheme {
  const dark = colorTheme === "dark";
  return {
    ...ANSI_COLORS,
    background: dark ? "#091110" : "#111a18",
    foreground: dark ? "#f2ead8" : "#f5efdf",
    cursor: dark ? "#e7b74e" : "#dba43a",
    cursorAccent: "#17201d",
    selectionBackground: dark ? "#6f876b73" : "#71896f80",
    selectionForeground: "#fff9e8",
    selectionInactiveBackground: dark ? "#46574766" : "#4e604f59",
  };
}

export function documentColorTheme(): ColorTheme {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}
