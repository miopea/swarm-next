import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import UsageSettings from "./UsageSettings";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const ok = (body: unknown) =>
  new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });

function report(overrides: Record<string, unknown> = {}) {
  return {
    days: 7,
    last_scan_at: Math.floor(Date.now() / 1000) - 600,
    scanning: false,
    by_workspace: [
      {
        workspace: "/w/queen",
        workers: ["Queen"],
        input_tokens: 10,
        cache_write_tokens: 1_000,
        cache_read_tokens: 99_000,
        output_tokens: 20,
        messages: 500,
        weighted: 700,
        weighted_previous: 600,
      },
      {
        workspace: "/w/platform",
        workers: ["Platform", "Platform · Codex"],
        input_tokens: 5,
        cache_write_tokens: 500,
        cache_read_tokens: 9_500,
        output_tokens: 10,
        messages: 100,
        weighted: 300,
        weighted_previous: 300,
      },
    ],
    by_day: [
      {
        day: "2026-09-13",
        input_tokens: 15,
        cache_write_tokens: 1_500,
        cache_read_tokens: 108_500,
        output_tokens: 30,
        weighted: 1_000,
      },
    ],
    total_weighted: 1_000,
    total_weighted_previous: 900,
    ...overrides,
  };
}

/**
 * ⚠️ THE PANEL SAYS HOW OLD IT IS, ALWAYS.
 *
 * These figures come from a scan of the provider's transcripts -- gigabytes on
 * a busy Hive, minutes to read. Asking schedules the next pass rather than
 * waiting for it, so what is drawn is always the LAST COMPLETED pass. A number
 * with no stated age gets trusted for longer than it deserves, and this one
 * would be read as live.
 */
test("the panel states its own freshness rather than implying it is live", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok(report())));

  render(<UsageSettings operatorToken="t" visible />);

  expect(await screen.findByText(/Measured 10m ago/)).toBeInTheDocument();
});

test("a Hive that has never been measured says so instead of reading as zero", async () => {
  vi.stubGlobal("fetch", vi.fn(async () =>
    ok(report({ last_scan_at: null, by_workspace: [], by_day: [], total_weighted: 0 })),
  ));

  render(<UsageSettings operatorToken="t" visible />);

  expect(await screen.findByText(/Not measured yet/)).toBeInTheDocument();
  // An empty panel and an unmeasured one look identical unless one of them
  // says which it is.
  expect(screen.getByText(/first pass reads every transcript once/)).toBeInTheDocument();
});

/**
 * ⚠️ A WORKSPACE CAN HOLD MORE THAN ONE WORKER, and the transcripts cannot tell
 * them apart -- the provider's directory is derived from the workspace PATH, so
 * Platform and Platform · Codex write into the same files. Naming one of them
 * would be a precision these numbers do not have.
 */
test("a workspace shared by two workers names both of them", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok(report())));

  render(<UsageSettings operatorToken="t" visible />);

  const rows = await screen.findAllByRole("listitem");
  expect(within(rows[1]).getByText("Platform, Platform · Codex")).toBeInTheDocument();
});

/**
 * The operator asked for cached and uncached to be told apart. The hit rate is
 * reads over reads-plus-writes: a high share is cheap burn, while writes
 * rivalling reads is a cache being rebuilt rather than reused.
 */
test("cached and uncached are separated, not summed", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok(report())));

  render(<UsageSettings operatorToken="t" visible />);

  // 108,500 read of 110,000 read-plus-written.
  expect(await screen.findByText("Cache hit 98.6%")).toBeInTheDocument();
  const table = screen.getByRole("table");
  expect(within(table).getByRole("columnheader", { name: "Cache read" })).toBeInTheDocument();
  expect(within(table).getByRole("columnheader", { name: "Cache write" })).toBeInTheDocument();
});

test("the window change is arithmetic against the preceding window of equal length", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok(report())));

  render(<UsageSettings operatorToken="t" visible />);

  // 1000 against 900.
  expect(await screen.findByText("+11%")).toBeInTheDocument();
  // And per workspace: 700 against 600.
  const rows = screen.getAllByRole("listitem");
  expect(within(rows[0]).getByText(/\+17%/)).toBeInTheDocument();
});

/**
 * A workspace with no previous window is new, not infinitely up. Dividing by
 * zero here would print "Infinity%" beside a real number and discredit the
 * whole panel.
 */
test("a workspace with no prior window shows a share without a change", async () => {
  vi.stubGlobal("fetch", vi.fn(async () =>
    ok(
      report({
        by_workspace: [
          {
            workspace: "/w/new",
            workers: ["Newcomer"],
            input_tokens: 1,
            cache_write_tokens: 1,
            cache_read_tokens: 1,
            output_tokens: 1,
            messages: 1,
            weighted: 1_000,
            weighted_previous: 0,
          },
        ],
      }),
    ),
  ));

  render(<UsageSettings operatorToken="t" visible />);

  const rows = await screen.findAllByRole("listitem");
  expect(within(rows[0]).getByText("100.0%")).toBeInTheDocument();
  expect(within(rows[0]).queryByText(/Infinity/)).not.toBeInTheDocument();
});

test("a failed read says so and keeps the panel recoverable", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("offline"); }));

  render(<UsageSettings operatorToken="t" visible />);

  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(/could not be read/));
});
