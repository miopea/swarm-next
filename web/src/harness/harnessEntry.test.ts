import { expect, test } from "vitest";
import cargoToml from "../../../Cargo.toml?raw";
import appEntry from "../main.tsx?raw";
import harnessEntry from "./main.tsx?raw";
import { nextMinor, workspaceVersion } from "./workspaceVersion";

/** Every bare `import "…";` in a module: the ones loaded for their effect. */
function sideEffectImports(source: string): string[] {
  return [...source.matchAll(/^import "([^"]+)";$/gm)].map((match) => match[1]);
}

/**
 * The harness renders the real components, and they are only the real
 * components with the stylesheets the app loads. The harness entry once lacked
 * both fonts and xterm.css, so every capture fell back to a system font and an
 * unstyled terminal, and the README pictures were taken that way.
 */
test("the harness loads every font and stylesheet the app loads, in the app's order", () => {
  const app = sideEffectImports(appEntry).map((path) => path.replace(/^\.\//, "../"));
  const harness = sideEffectImports(harnessEntry);

  expect(app.length).toBeGreaterThan(0);
  expect(harness).toEqual(app);
});

test("the harness reads the version from the workspace manifest", () => {
  expect(workspaceVersion(cargoToml)).toMatch(/^\d+\.\d+\.\d+$/);
  expect(
    workspaceVersion('[package]\nversion = "0.1.0"\n\n[workspace.package]\nversion = "2.3.4"\nedition = "2024"\n'),
  ).toBe("2.3.4");
  expect(() => workspaceVersion('[package]\nversion = "0.1.0"\n')).toThrow();
});

test("an offered update is newer than the version the harness runs", () => {
  expect(nextMinor("1.17.10")).toBe("1.18.0");
});
