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
  expect(stylesheet).toContain(".header-actions .icon-button { width: var(--pill-touch-height); height: var(--pill-touch-height); min-height: var(--pill-touch-height); }");
  expect(stylesheet).toContain(".header-actions .secondary-button { height: var(--pill-touch-height); min-height: var(--pill-touch-height);");
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
  expect(stylesheet).toContain(".worker-state-with_operator { --worker-state: var(--with-operator); }");
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

test("spans a revealed task panel across the card instead of into a column", () => {
  // The Original report and Step 1 of 2 panels drew on top of each other and
  // clipped at the card's right edge. Both are wrapped in a plain div so they
  // can be hidden without unmounting, and that wrapper is the grid item — the
  // span declared on the panel inside it does nothing, so the wrapper was
  // auto-placed into one of the card's named columns.
  expect(stylesheet).toContain(".task-card-panel { grid-column: 1 / -1; min-width: 0; }");
});

test("keeps with-you and awaiting-you from reading as the same state", () => {
  // Raised as: "with you" needs another colour, right now it is the same as
  // waiting. It was — both were amber, 12.5 dE apart in light and 5.0 in dark,
  // which is close to no difference at all. They mean opposite things: one is
  // the operator being present, the other is a worker stopped waiting for them.
  expect(stylesheet).toContain("--with-operator: #6f4bb8;");
  expect(stylesheet).toContain("--with-operator: #b79bf0;");
  // Borrowing the accent is what put it next to the warning tone.
  expect(stylesheet).not.toContain(".worker-state-with_operator { --worker-state: var(--accent-strong); }");
});

test("gives the worker picker as many rows as it has children, so its footer survives", () => {
  // Reported from a phone: "The bottom 'Manage Workers' is cut off and I can't
  // scroll up to see the button."
  //
  // The dialog has four children — heading, toolbar, list, footer button — and
  // the template declared five rows. The list therefore landed in an `auto`
  // row, which sizes to content and will not shrink, so a long roster grew past
  // the dialog's max-height and pushed the footer out of a box that clips its
  // overflow. The list's own `overflow-y: auto` never engaged because the row
  // gave it all the height it asked for.
  const dialog = /\.mobile-worker-dialog \{[^}]*\}/.exec(stylesheet)?.[0] ?? "";
  const rows = /grid-template-rows:\s*([^;]+);/.exec(dialog)?.[1].trim();

  expect(rows).toBe("auto auto minmax(0, 1fr) auto");
  // The list is the row that gives way, and it scrolls rather than clipping.
  expect(stylesheet).toContain(".mobile-worker-dialog-list { display: grid; min-height: 0; overflow-y: auto;");
});

test("renders every header pill at one height, and lets the chrome yield before the selector", () => {
  // Measured against the deployed stylesheet in a real layout engine, at real
  // phone widths, before this rule existed:
  //
  //   heights >=768px : 21, 26, 34, 38, 39  — five different pills
  //   header-actions  : a rigid 252px at 360, 390, 414, 600 and 680
  //   selector width  : 84px at 360px, with the worker name AND the task clipped
  //
  // So "the selector gets bumped over by the open pill" is the wrong element
  // being inflexible. The chrome beside it — presence, four icon buttons, Lock
  // — never gave way, and the one control the screen exists for absorbed all
  // of the loss.
  const pillHeight = /--pill-height:\s*([^;]+);/.exec(stylesheet)?.[1].trim();
  expect(pillHeight).toBeTruthy();

  // Every pill-shaped control is sized by the token rather than by its content.
  for (const pill of [
    ".worker-context-task",
    ".worker-repository",
    ".worker-context-queue",
    ".operator-presence-chip",
  ]) {
    expect(stylesheet).toMatch(
      new RegExp(`${pill.replace(".", "\\.")}[^{]*\\{[^}]*min-height:\\s*var\\(--pill-height\\)`),
    );
  }

  // Same height for the buttons sharing that row, which is what the operator
  // was pointing at.
  expect(stylesheet).toMatch(
    /\.header-actions \.icon-button, \.header-actions \.secondary-button \{[^}]*height:\s*var\(--pill-height\)/,
  );

  // The selector takes the room; the chrome never squeezes to give it. Making
  // the row shrinkable was tried and measured: it took the icon buttons from
  // 44px to 15px wide, which is a worse defect than the crowding it relieved.
  expect(stylesheet).toMatch(/\.header-actions > \* \{[^}]*flex:\s*0 0 auto/);
  expect(stylesheet).toMatch(/\.mobile-worker-switcher-trigger \{[^}]*flex:\s*1 1 auto/);
});

test("keeps the age legible on a state that paints its own pill", () => {
  // Reported: "when buzzing the time is hard to read." The age was written in a
  // fixed --quiet grey, which is chosen against the panel — but buzzing fills
  // the pill with --busy and writes on it in --panel. Grey on green measured
  // 1.65:1 in light and 1.71:1 in dark, against the 4.5:1 this text size needs.
  //
  // A tint of the actual foreground is legible on every state by construction,
  // because it is always a shade of whatever is already readable there.
  expect(stylesheet).toContain(".worker-silence { color: inherit; opacity: .85;");
  expect(stylesheet).not.toMatch(/\.worker-silence \{ color: var\(--quiet\)/);
});

test("pins the runtime footer to the bottom of the rail on every surface", () => {
  // It sat under whatever happened to be above it, so on surfaces with no rail
  // context it floated partway up the column and the runtime lived somewhere
  // different depending on which screen you were on.
  expect(stylesheet).toMatch(/\.rail-footer \{[^}]*margin-top:\s*auto/);
});

test("keeps settings navigable on a phone once its own bar is gone", () => {
  // Settings sections moved into the rail to match every other surface. The
  // rail context is hidden at phone width, so without this the only way to
  // reach a settings section on a phone would have disappeared with the bar.
  expect(stylesheet).toContain(".control-rail.surface-settings .rail-context { display: flex;");
  expect(stylesheet).toMatch(/\.rail-settings-sections \{[^}]*overflow-x:\s*auto/);
});

test("lets a long task status wrap in its own cell instead of crossing the next one", () => {
  // Reported as the finished-unverified row being "a mess". Measured against the
  // deployed stylesheet in a real layout engine: "Finished · unverified" is 151px
  // of text that white-space: nowrap forbids breaking, inside a 59px cell, so it
  // overlapped the Priority label by 36px and its value by 42px — at every panel
  // width, and it spilled past the panel too. After: zero overlaps at 160 to 320.
  expect(stylesheet).toContain(".task-state, .task-priority { min-width: 0; white-space: normal; overflow-wrap: anywhere; }");
  expect(stylesheet).toContain(".task-state::before { flex: 0 0 auto; }");
});
