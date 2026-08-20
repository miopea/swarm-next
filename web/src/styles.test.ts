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
  // The phone picker has no search field: it took focus on open and raised the
  // keyboard over the roster the operator had just asked to see.
  expect(stylesheet).not.toContain(".mobile-worker-search");
  expect(stylesheet).toContain(".mobile-worker-empty button { min-height: 44px;");
  expect(stylesheet).toContain('.settings-workspace input:not([type="checkbox"]):not([type="radio"]),');
  expect(stylesheet).toContain(".settings-workspace textarea { min-height: 44px; }");
  expect(stylesheet).toContain(".task-mobile-controls summary { display: flex; min-height: 44px;");
  expect(stylesheet).toContain(".task-mobile-controls input, .task-mobile-controls select { width: 100%; min-height: 44px;");
});

test("keeps the roster controls still while only the worker list scrolls", () => {
  expect(stylesheet).toContain(
    ".rail-context { display: flex; min-height: 0; flex: 1; flex-direction: column; padding-top: 17px; overflow: hidden; }",
  );
  expect(stylesheet).toContain(".rail-controls { flex: 0 0 auto; }");
  expect(stylesheet).toContain(
    ".rail-context .worker-list, .rail-context .empty-rail { min-height: 0; overflow-y: auto; overscroll-behavior: contain; }",
  );
  // Board view has no list under its controls, so there the controls scroll.
  expect(stylesheet).toContain(
    ".control-rail.surface-tasks .rail-controls { min-height: 0; flex: 1; overflow-y: auto; }",
  );
});

test("gives the phone terminal its row back when the toolbar has nothing to report", () => {
  expect(stylesheet).toContain(".terminal-toolbar-quiet { display: none; }");
  // The folded controls belong to the phone layout only.
  expect(stylesheet).toContain(".terminal-connection-dot, .terminal-sleep-button { display: none; }");
  expect(stylesheet).toContain(".terminal-sleep-button { display: inline-flex; color: var(--bad); }");
});

test("keeps the phone terminal free of context chrome it does not need", () => {
  // The phone reclaimed a row of chrome; a bar of context chips would give it
  // straight back. Only the fact a phone cannot deduce survives: that another
  // device is driving this worker, and so owns its terminal width.
  expect(stylesheet).toContain(
    ".worker-context-task, .worker-context-queue, .worker-repository { display: none; }",
  );
  expect(stylesheet).toContain(".header-actions .popout-button { display: none; }");
});

test("reads worker state as a scale rather than a palette", () => {
  // Green is work happening, amber is work stopped and wanting the operator,
  // red is work that cannot proceed. Previously resting held the green and
  // buzzing the amber, and the two states most needing to differ — waiting on
  // the operator, and held by the operator — shared one colour.
  expect(stylesheet).toContain(".worker-state-buzzing { --worker-state: var(--busy); }");
  expect(stylesheet).toContain(".worker-state-awaiting_operator { --worker-state: var(--warn); }");
  expect(stylesheet).toContain(".worker-state-with_operator { --worker-state: var(--accent-strong); }");
  expect(stylesheet).toContain(".worker-state-blocked { --worker-state: var(--bad); }");
  // Sleeping and resting are separated by fill, not hue, so the distinction
  // survives where colour does not.
  expect(stylesheet).toContain(
    ".worker-row.worker-state-sleeping .presence { background: transparent;",
  );
});

test("moves only the worker state that is actually doing something", () => {
  expect(stylesheet).toContain(
    ".worker-row.worker-state-buzzing .presence { animation: worker-buzz 1.8s ease-in-out infinite; }",
  );
  // Motion is never the only signal, and never forced on someone who asked for less.
  expect(stylesheet).toContain(".worker-row.worker-state-buzzing .presence { animation: none; }");
});

test("separates a working worker from a resting one by more than a shade", () => {
  // Raised as: buzzing and resting look almost alike, impossible to notice a
  // difference when scanning. Measured at the time: 14.6 dE apart in light and
  // 13.3 in dark, on uppercase text at .58rem inside a 12% tint.
  //
  // --good is a soft success tone that sits right next to the resting grey, so
  // buzzing gets its own colour rather than borrowing it.
  expect(stylesheet).toContain(".worker-state-buzzing { --worker-state: var(--busy); }");
  expect(stylesheet).toContain("--busy: #2f6b2a;");
  expect(stylesheet).toContain("--busy: #7fd070;");

  // And the difference does not rest on hue alone, for the same reason sleeping
  // is a hollow dot rather than another shade.
  expect(stylesheet).toContain(
    ".worker-row.worker-state-buzzing .worker-attention-label { color: var(--panel); background: var(--busy); }",
  );
});
