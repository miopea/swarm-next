export type ColorTheme = "light" | "dark";

const STORAGE_KEY = "swarm-next.color-theme.v1";

export function initialColorTheme(): ColorTheme {
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved === "light" || saved === "dark") return saved;
  } catch {
    // A usable theme does not depend on browser storage.
  }
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function applyColorTheme(theme: ColorTheme): void {
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
  try {
    window.localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // The in-memory choice still applies when storage is unavailable.
  }
}
