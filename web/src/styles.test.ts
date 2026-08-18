// Vitest runs this file in Node even though the web TypeScript project does not
// otherwise expose Node types.
// @ts-expect-error Node's test-only filesystem module is intentionally untyped here.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

declare const process: { cwd(): string };

const stylesheet = readFileSync(`${process.cwd()}/src/styles.css`, "utf8");

test("defines every design token referenced by the application stylesheet", () => {
  const defined = new Set([...stylesheet.matchAll(/(--[a-z0-9-]+)\s*:/gi)].map((match) => match[1]));
  const referenced = new Set([...stylesheet.matchAll(/var\((--[a-z0-9-]+)/gi)].map((match) => match[1]));
  const missing = [...referenced].filter((token) => !defined.has(token)).sort();

  expect(missing).toEqual([]);
});

test("adapts task rows to the board width left by the resizable worker rail", () => {
  expect(stylesheet).toContain("container-name: task-board");
  expect(stylesheet).toContain("@container task-board (max-width: 920px)");
  expect(stylesheet).toContain('"assignment actions"');
});

test("keeps the mobile settings section rail flush with its scroll viewport", () => {
  expect(stylesheet).toContain(
    ".settings-section-nav { top: 0; margin-block-start: -12px; margin-inline: -4px; padding: 7px; }",
  );
});

test("lets repository paths wrap inside the worker editor", () => {
  expect(stylesheet).toContain(".worker-repository-path code { min-width: 0; overflow-wrap: anywhere;");
});
