import { expect, test } from "vitest";

import controllerSource from "./TerminalController.ts?raw";

/**
 * The mutating fit() has exactly one caller, and that caller knows the rule.
 *
 * WHY THIS IS A SOURCE TEST AND NOT A BEHAVIOUR TEST. The rule is "a device
 * that does not own the geometry never mutates its local grid". A behaviour
 * test can only assert it about a path it thought to exercise, and that is
 * precisely how this defect shipped twice: aa4de4a taught the rule to one call
 * site, e088777 taught it to a second, and the operator's phone was hitting a
 * third. Each fix had a passing test. Each test measured the call site the bug
 * report had arrived through.
 *
 * An invariant stated at N call sites will be taught to one of them. So the
 * check is not "does this path obey the rule" but "is there still only one
 * place that could disobey it".
 *
 * IT FOUND ONE IMMEDIATELY. When first written this failed at 2 — the snapshot
 * handler's owning branch called fit() directly. That call was CORRECT: it had
 * already established the device owns the geometry. It was still a second place
 * that would not be told when the rule changed.
 *
 * WHAT IT CANNOT CATCH, said plainly because a guard whose limits are unstated
 * gets trusted past them: it counts text. A caller reached through an alias, a
 * destructured method or a dynamic property will not be seen, and it says
 * nothing about whether #measureForResize's own logic is right — that is what
 * the behaviour tests beside it are for. It closes one door, not the corridor.
 */
test("only #measureForResize may mutate the terminal grid", () => {
  const controller = controllerSource;

  // Deliberately counts the raw call, comments and all: a commented-out second
  // caller is a second caller waiting to be uncommented.
  const callers = controller.split("this.#surface.fit()").length - 1;

  expect(
    callers,
    "the mutating fit() must be reached only through #measureForResize, which is "
      + "the one place that checks whether this device owns the geometry",
  ).toBe(1);

  // And that one call must be inside the helper rather than merely somewhere in
  // the file, or the count above would pass while the rule moved out from under
  // it.
  const helper = controller.slice(controller.indexOf("async #measureForResize("));
  const helperBody = helper.slice(0, helper.indexOf("\n  }"));
  expect(helperBody).toContain("this.#surface.fit()");
  expect(helperBody).toContain("ownsGeometry");
});
