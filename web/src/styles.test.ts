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

/**
 * SECONDARY TEXT MUST CLEAR AA ON EVERY GROUND IT LANDS ON, NOT JUST THE ONE
 * SOMEBODY CHECKED.
 *
 * `--muted` measured 4.98 on `--panel` and passed every casual inspection,
 * while failing on the two softer grounds it is used over just as often: 4.41
 * on `--panel-soft` and 4.49 on the attention card's honey wash. A value that
 * is correct on the background you happen to test is the reason this survived.
 *
 * The wash is not a token — it is `color-mix(--accent-soft 45%, --panel)` — so
 * it is recomputed here rather than read, which is also what makes this test
 * fail if either ingredient moves.
 */
test("muted secondary text clears 4.5:1 on every light ground it is used over", () => {
  const token = (name: string) => {
    const match = stylesheet.match(new RegExp(`\\n  ${name}:\\s*(#[0-9a-f]{6})`, "i"));
    if (!match) throw new Error(`${name} is not defined as a literal in :root`);
    return match[1];
  };
  const channels = (hex: string) =>
    [1, 3, 5].map((at) => Number.parseInt(hex.slice(at, at + 2), 16));
  const luminance = (hex: string) => {
    const [r, g, b] = channels(hex)
      .map((value) => value / 255)
      .map((value) => (value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const contrast = (a: string, b: string) => {
    const [x, y] = [luminance(a), luminance(b)];
    return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
  };
  const mix = (a: string, b: string, share: number) =>
    `#${channels(a)
      .map((value, index) => Math.round(share * value + (1 - share) * channels(b)[index]))
      .map((value) => value.toString(16).padStart(2, "0"))
      .join("")}`;

  const muted = token("--muted");
  const panel = token("--panel");
  const grounds = {
    "--panel": panel,
    "--panel-soft": token("--panel-soft"),
    // The attention card's ground, as .apiary-attention-card composites it.
    "honey wash": mix(token("--accent-soft"), panel, 0.45),
  };

  for (const [name, ground] of Object.entries(grounds)) {
    expect(
      contrast(muted, ground),
      `--muted on ${name} must clear 4.5:1`,
    ).toBeGreaterThanOrEqual(4.5);
  }
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

test("gives the attention region no height of its own when it is empty", () => {
  // Item 48's second door reserved a card's height whether or not a card was in
  // it, so mounting one could not shove the list. That left a permanent blank
  // band above the queue on every card-free day, which is most of them, and the
  // operator asked for it to go. An appearing card shifts the list again; that
  // is the trade, made deliberately.
  expect(stylesheet).toContain(".decision-attention-cards:empty { display: none; }");
  expect(stylesheet).not.toContain(".decision-attention-cards.reserved { min-height");
});

test("keeps the worker-stopping update badge legible", () => {
  // --warn on --warn-soft measures 1.93:1 in the light theme, well under AA's
  // 4.5:1 for this badge's small uppercase text. --warn-strong is the text
  // colour; --warn stays a border and icon colour.
  expect(stylesheet).toContain(".runtime-status-badge.attention { color: var(--warn-strong); background: var(--warn-soft); }");
  expect(stylesheet).toContain("--warn-strong: #7d5a0d;");
});

test("lets a repository path be edited without stretching the worker editor", () => {
  // A worker is moved by typing a path, and a long one must not widen the row
  // past its column — the grid child needs min-width: 0 for that.
  expect(stylesheet).toContain(".worker-repository-field input { min-width: 0; overflow-wrap: anywhere;");
  expect(stylesheet).toContain(".worker-repository-field small { min-width: 0;");
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
  // ON A PSEUDO-ELEMENT SINCE 2026-09-02, and that is the point rather than a
  // detail. The rule animated `box-shadow` on the dot itself, which is not a
  // composited property, so every buzzing row repainted sixty times a second for
  // as long as the tab was open — measured by the operator at 15% CPU in Edge.
  // The ring is now its own element moved by transform and opacity, which the
  // compositor handles without painting. compositedAnimations.test.ts enforces
  // that for every looping animation; this asserts the pulse still exists.
  expect(stylesheet).toContain(".worker-row.worker-state-buzzing .presence::after {");
  expect(stylesheet).toContain("animation: worker-buzz 1.8s ease-in-out infinite;");
  // Motion is never the only signal, and never forced on someone who asked for less.
  expect(stylesheet).toContain(
    ".worker-row.worker-state-buzzing .presence::after { animation: none; }",
  );
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

test("offers no detaching on a phone, and keeps the header's own text centred", () => {
  // Two regressions from giving these controls a fixed height and a touch size.
  //
  // A phone cannot usefully open a second window, so the pop-out controls do
  // not belong in its navigation row — making them permanently visible because
  // touch has no hover was the wrong answer to the right observation.
  // Scoped to the nav, and that scoping is the point rather than a detail:
  // this control is a button inside `.surface-nav`, whose own rule sets display
  // at higher specificity. The unscoped version lost to it at every width, so
  // the pop-out kept showing on phones and the rule looked correct in the file.
  expect(stylesheet).toContain(".surface-nav .surface-nav-popout { display: none; }");

  // And a fixed height on a block button leaves its text wherever the padding
  // put it: Lock's label measured 3.5px below the centre of its own box.
  expect(stylesheet).toContain(".header-actions .secondary-button { display: inline-flex; align-items: center; justify-content: center; }");
});

test("a detached window on a phone is the whole width", () => {
  // Measured against the deployed stylesheet: a popped-out window at 390px gave
  // its rail 260px and the workspace 130px — a third of the screen, with the
  // rail empty because its context is hidden at that width. The rail earns its
  // place on a desktop, where it carries the worker picker; on a phone that
  // picker is already in the header, so the rail is only cost.
  expect(stylesheet).toContain(".app-shell.detached-surface { grid-template-columns: minmax(0, 1fr); }");
  expect(stylesheet).toContain(".control-rail.detached-rail { display: none; }");
});

/**
 * The held-work card has no bee, so it cannot inherit the three-column grid its
 * siblings use. Without its own track list the text was laid into the 44px icon
 * column and wrapped one word per line, which is what the operator saw on the
 * "Needs you" page.
 */
test("gives the held-work card a track list matching its two children", () => {
  expect(stylesheet).toContain(".held-delivery-card { grid-template-columns: minmax(0, 1fr) auto; }");
  expect(stylesheet).toContain(".held-delivery-card { grid-template-columns: minmax(0, 1fr); }");
});

/**
 * The Needs-you count must be legible, which is a stricter bar than AA.
 *
 * It sat at 5.25:1 — over the 4.5:1 AA bar for normal text — and the operator
 * still reported it as hard to read. The ratio was never the whole story: the
 * count renders at .7rem, and .63rem in the narrow rail, so it is an ~11px bold
 * numeral whose ink and ground share a hue. WCAG's formula does not model that,
 * and a badge nobody can read is a badge that stops being checked, which
 * defeats the one thing this queue has to do.
 *
 * So this asserts 7:1 rather than 4.5:1, in BOTH themes. The second half
 * matters independently: the previous pairing was 5.25:1 in light and 10:1 in
 * dark, leaving one theme half as legible as the other with nothing to say so.
 */
test("the Needs-you count clears a small-text bar in both themes, not merely AA", () => {
  const channel = (value: number) => {
    const c = value / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const luminance = (hex: string) => {
    const value = hex.replace("#", "");
    const [r, g, b] = [0, 2, 4].map((at) => parseInt(value.slice(at, at + 2), 16));
    return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
  };
  const contrast = (a: string, b: string) => {
    const [high, low] = [luminance(a), luminance(b)].sort((first, second) => second - first);
    return (high + 0.05) / (low + 0.05);
  };
  const token = (name: string, from: string) => {
    const found = from.match(new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{6})`));
    if (!found) throw new Error(`--${name} must be a literal colour so its contrast can be checked`);
    return found[1];
  };
  // This app stamps the theme explicitly rather than reading the OS, so the
  // dark palette lives under a selector, not a media query.
  const darkAt = stylesheet.indexOf(':root[data-theme="dark"]');
  expect(darkAt).toBeGreaterThan(0);
  const lightBlock = stylesheet.slice(0, darkAt);
  const darkBlock = stylesheet.slice(darkAt);

  const light = contrast(token("on-accent", lightBlock), token("attention-badge", lightBlock));
  const dark = contrast(token("on-accent", darkBlock), token("attention-badge", darkBlock));

  expect(light).toBeGreaterThanOrEqual(7);
  expect(dark).toBeGreaterThanOrEqual(7);
  // Neither theme may be markedly worse than the other. The old pairing passed
  // AA in both and was still twice as legible in one as in the other.
  expect(Math.min(light, dark) / Math.max(light, dark)).toBeGreaterThan(0.6);
});

// THE WIDTH RULE HAS TO WIN, NOT MERELY EXIST. `.whats-new { width: min(760px,
// 100%) }` sat above `.dialog { width: min(440px, 100%) }` with the same
// specificity, so the later rule won and the panel rendered at 440px for its
// whole life. Nothing failed: the rule was present, correct, and dead.
test("gives the What's New panel a width that outranks the base dialog", () => {
  const widthOf = (selector: string) =>
    stylesheet
      .split("\n")
      .find((line: string) => line.trimStart().startsWith(`${selector} {`))
      ?.match(/width:\s*([^;]+);/)?.[1]
      ?.trim();

  const base = widthOf(".dialog");
  const panel = widthOf(".dialog.whats-new");
  expect(base).toBeDefined();
  expect(panel).toBeDefined();

  // Qualified with .dialog, so it does not depend on where it sits in the file.
  // The bare single-class form is what lost; if it returns, so does the bug.
  expect(stylesheet).not.toMatch(/^\.whats-new\s*\{[^}]*width:/m);

  const pixels = (rule: string) => Number(rule.match(/(\d+)px/)?.[1]);
  expect(pixels(panel as string)).toBeGreaterThan(pixels(base as string));
});

// <strong> WAS NOT ENOUGH. 1.0.0's bullets rendered real <strong> elements that
// looked exactly like body text, because the global reset sets strong to 500
// against a 400 body. A test asserting the TAG passes while the screen is
// unchanged -- this asserts the weight that makes it visible.
test("makes emphasis in What's New heavier than the global strong reset", () => {
  const globalStrong = stylesheet.match(/^h1, h2, h3, h4, strong, button \{[^}]*font-weight:\s*(\d+)/m)?.[1];
  const panelStrong = stylesheet.match(/^\.dialog\.whats-new strong \{[^}]*font-weight:\s*(\d+)/m)?.[1];
  expect(globalStrong).toBeDefined();
  expect(panelStrong).toBeDefined();
  expect(Number(panelStrong)).toBeGreaterThan(Number(globalStrong));
});
