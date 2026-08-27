import { describe, expect, test } from "vitest";

import { anyAwaitingWorkerEngine, releasesNewerThan, whatsNewFor } from "./whatsNew";
import type { ReleaseVersionNotes } from "../api";

const note = (summary: string, engine = false) => ({ summary, kind: "feature", needs_worker_engine_update: engine });
const release = (version: string, engine = false): ReleaseVersionNotes => ({ version, notes: [note(`${version} changed`, engine)] });

describe("what's new", () => {
  /** The common case: away for a day, several releases behind. */
  test("shows every release the operator skipped, newest first", () => {
    const releases = [release("0.8.16"), release("0.8.19"), release("0.8.17"), release("0.8.14")];
    expect(releasesNewerThan(releases, "0.8.15").map((entry) => entry.version)).toEqual(["0.8.19", "0.8.17", "0.8.16"]);
  });

  /**
   * The pair that defeats string comparison. "0.8.9" sorts AFTER "0.8.10"
   * lexically, so a Hive crossing that rollover would be told it was ahead.
   */
  test("compares versions numerically, not as strings", () => {
    expect(releasesNewerThan([release("0.8.10")], "0.8.9").map((entry) => entry.version)).toEqual(["0.8.10"]);
    expect(releasesNewerThan([release("0.8.9")], "0.8.10")).toEqual([]);
  });

  /** A fresh install has not missed anything and is not shown a changelog. */
  test("a first run records the version and shows nothing", () => {
    const result = whatsNewFor([release("0.8.19"), release("0.8.18")], "0.8.19", null);
    expect(result.show).toEqual([]);
    expect(result.recordAs).toBe("0.8.19");
  });

  test("an operator who is already current is shown nothing and nothing is rewritten", () => {
    const result = whatsNewFor([release("0.8.19")], "0.8.19", "0.8.19");
    expect(result.show).toEqual([]);
    expect(result.recordAs).toBeNull();
  });

  test("an unreadable stored version shows nothing rather than guessing", () => {
    expect(releasesNewerThan([release("0.8.19")], "not-a-version")).toEqual([]);
    expect(releasesNewerThan([release("0.8.19")], "")).toEqual([]);
  });

  /**
   * The caveat that matters: the terminal host is a separate service, so a
   * host-side change is installed and NOT in effect. Announcing it as available
   * would be a confident false claim about what the operator can now do.
   */
  test("a host-side change stays flagged as not yet in effect", () => {
    expect(anyAwaitingWorkerEngine([release("0.8.19", true)])).toBe(true);
    expect(anyAwaitingWorkerEngine([release("0.8.19", false)])).toBe(false);
  });
});

test("an operator whose browser has never seen the panel is still told what changed", () => {
  // THE ABLATION. Drop the `previous` argument and this shows nothing, because
  // an empty local storage is indistinguishable from a first install without
  // it. That is the case a second machine, a private window, and cleared site
  // data all land in.
  const result = whatsNewFor(
    [release("0.8.19"), release("0.8.18"), release("0.8.17"), release("0.8.16")],
    "0.8.19",
    null,
    "0.8.16",
  );
  expect(result.show.map((entry) => entry.version)).toEqual(["0.8.19", "0.8.18", "0.8.17"]);
  expect(result.recordAs).toBe("0.8.19");
});

test("a genuine first install is still shown nothing", () => {
  const result = whatsNewFor([release("0.8.19"), release("0.8.18")], "0.8.19", null, null);
  expect(result.show).toEqual([]);
  expect(result.recordAs).toBe("0.8.19");
});

test("the anchor reaching further back wins, so a note is not skipped", () => {
  // Dismissed on another device at 0.8.18 while this Hive came from 0.8.16:
  // the three releases since 0.8.16 are what this operator has not seen here.
  const result = whatsNewFor(
    [release("0.8.19"), release("0.8.18"), release("0.8.17")],
    "0.8.19",
    "0.8.18",
    "0.8.16",
  );
  expect(result.show.map((entry) => entry.version)).toEqual(["0.8.19", "0.8.18", "0.8.17"]);
});

test("a gap deeper than the notes carry is reported, not presented as complete", () => {
  const result = whatsNewFor([release("0.8.19"), release("0.8.18")], "0.8.19", null, "0.8.10");
  expect(result.show.map((entry) => entry.version)).toEqual(["0.8.19", "0.8.18"]);
  expect(result.truncated).toBe(true);
});

test("a gap the notes cover completely is not reported as truncated", () => {
  const result = whatsNewFor([release("0.8.19"), release("0.8.18")], "0.8.19", null, "0.8.18");
  expect(result.truncated).toBe(false);
});
