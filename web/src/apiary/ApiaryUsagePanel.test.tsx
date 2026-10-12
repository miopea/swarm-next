import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import ApiaryUsagePanel from "./ApiaryUsagePanel";

const totals = (weighted: number, active_seconds: number) => ({ input_tokens: 0, cache_write_tokens: 0, cache_read_tokens: 0, output_tokens: 0, messages: 0, weighted, active_seconds });
const names: Record<string, string> = { meadow: "Meadow Hive", clover: "Clover Hive", heather: "Heather Hive" };

/** ⚠️ "Time or token based or both": both, per Hive and for the Apiary, biggest spender first. */
test("the Apiary's spend reads by Hive, by model and in total, and a Hive with no figures says so", async () => {
  const usage = {
      days: 7,
      hives: [
        { hive_id: "meadow", is_local: true, received_at: null, totals: totals(2_000_000, 7_200), by_model: [], by_workspace: [{ name: "/home/bea/queen", ...totals(2_000_000, 7_200) }] },
        { hive_id: "clover", is_local: false, received_at: 1_790_000_000, totals: totals(6_000_000, 5_400), by_model: [], by_workspace: [{ name: "/home/cora/budgetbug", ...totals(6_000_000, 5_400) }] },
        { hive_id: "heather", is_local: false, received_at: null, totals: totals(0, 0), by_model: [], by_workspace: [] },
      ],
      totals: totals(8_000_000, 12_600),
      by_model: [{ name: "claude-opus-5-5", ...totals(6_000_000, 0) }, { name: "claude-sonnet-5-5", ...totals(2_000_000, 0) }],
  };
  const onDays = vi.fn();
  render(<ApiaryUsagePanel usage={usage} days={7} onDays={onDays} nameFor={(hiveId) => names[hiveId]} />);

  const table = screen.getByRole("table");
  const rows = within(table).getAllByRole("row");
  const read = (row: HTMLElement) => [row.querySelector("summary")?.textContent ?? row.querySelector("th")?.textContent, ...within(row).getAllByRole("cell").map((cell) => cell.textContent)];
  expect(read(rows[1])).toEqual(["Clover Hive", "6.0M", "1h 30m"]);
  expect(read(rows[2])).toEqual(["Meadow Hive (this Hive)", "2.0M", "2h"]);
  expect(rows[3]).toHaveTextContent("No figures yet");
  expect(read(rows[3]).slice(1)).toEqual(["—", "—"]);
  expect(read(rows[4])).toEqual(["Apiary", "8.0M", "3h 30m"]);
  // Each Hive opens to its workspaces.
  expect(within(rows[1]).getByText("budgetbug")).toBeInTheDocument();
  expect(screen.getByRole("list", { name: "Spend by model" })).toHaveTextContent("claude-opus-5-56.0M75%claude-sonnet-5-52.0M25%");

  expect(screen.getByRole("button", { name: "7 days" })).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(screen.getByRole("button", { name: "30 days" }));
  expect(onDays).toHaveBeenCalledWith(30);
});

test("figures that could not be read say so rather than reading as zero", () => {
  render(<ApiaryUsagePanel usage={undefined} days={7} onDays={vi.fn()} nameFor={() => undefined} />);
  expect(screen.getByText(/Usage unavailable/)).toBeInTheDocument();
  expect(screen.queryByRole("table")).not.toBeInTheDocument();
});
