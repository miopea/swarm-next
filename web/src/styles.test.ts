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
    ".settings-workspace { width: 100%; min-width: 0; grid-template-columns: minmax(0, 1fr); padding: 0 12px 12px; overflow-x: hidden; }",
  );
  expect(stylesheet).toContain(".settings-section-nav { top: 0; margin-inline: -4px; padding: 7px; }");
});

test("lets repository paths wrap inside the worker editor", () => {
  expect(stylesheet).toContain(".worker-repository-path code { min-width: 0; overflow-wrap: anywhere;");
});

test("keeps Queen autonomy explanations readable on phones", () => {
  const baseRule = ".queen-policy-explainer { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr));";
  const mobileRule = ".queen-policy-explainer { grid-template-columns: minmax(0, 1fr); }";

  expect(stylesheet.lastIndexOf(mobileRule)).toBeGreaterThan(stylesheet.lastIndexOf(baseRule));
});

test("keeps persistent phone navigation controls comfortably touchable", () => {
  expect(stylesheet).toContain(".surface-nav button { min-width: 0; min-height: 44px;");
  expect(stylesheet).toContain(".header-actions .icon-button { width: 44px; min-height: 44px; }");
  expect(stylesheet).toContain(".header-actions .secondary-button { min-height: 44px;");
  expect(stylesheet).toContain(".operator-presence-chip { width: 44px;");
  expect(stylesheet).toContain(".attention-tabs button, .decision-actions button,");
  expect(stylesheet).toContain(".queen-autonomy-chip, .queen-automation-chip,");
  expect(stylesheet).toContain(".settings-section-nav button, .worker-edit-button,");
  expect(stylesheet).toContain(".task-entry-actions button, .task-control-heading button,");
  expect(stylesheet).toContain(".task-project-controls > button, .task-project-row > button,");
  expect(stylesheet).toContain(".task-project-row > a { display: grid; width: 44px; min-height: 44px;");
  expect(stylesheet).toContain(".task-jira-origin a { min-height: 44px; }");
  expect(stylesheet).toContain(".worker-order-actions button { width: 44px; min-height: 44px; }");
  expect(stylesheet).toContain(".mobile-terminal-keys button { min-width: 44px; min-height: 44px;");
  expect(stylesheet).toContain(".mobile-worker-search { width: 100%; min-height: 44px; }");
  expect(stylesheet).toContain(".mobile-worker-empty button { min-height: 44px;");
  expect(stylesheet).toContain('.settings-workspace input:not([type="checkbox"]):not([type="radio"]),');
  expect(stylesheet).toContain(".settings-workspace textarea { min-height: 44px; }");
  expect(stylesheet).toContain(".task-mobile-controls summary { display: flex; min-height: 44px;");
  expect(stylesheet).toContain(".task-mobile-controls input, .task-mobile-controls select { width: 100%; min-height: 44px;");
});
