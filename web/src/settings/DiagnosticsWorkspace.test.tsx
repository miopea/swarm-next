import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import DiagnosticsWorkspace from "./DiagnosticsWorkspace";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("retries saved dogfood reports without disturbing live diagnostics", async () => {
  let reportAttempts = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    if (String(input).includes("feedback/reports")) {
      reportAttempts += 1;
      if (reportAttempts === 1) return new Response("unavailable", { status: 502 });
      return new Response(JSON.stringify([]), { status: 200, headers: { "Content-Type": "application/json" } });
    }
    return new Response("unavailable", { status: 503 });
  }));

  render(<DiagnosticsWorkspace feedbackRevision={0} operatorToken="secret" health={undefined} hiveIdentity={undefined} liveFeedState="retrying" recentEvents={[]} sessions={[]} workers={[]} jiraReadiness={undefined} jiraUnavailable={true} />);

  expect(await screen.findByText("Saved reports are unavailable right now.")).toBeInTheDocument();
  expect(screen.getByText("API").parentElement).toHaveTextContent("Unavailable");
  fireEvent.click(screen.getByRole("button", { name: "Retry saved reports" }));
  await waitFor(() => expect(screen.getByText("No reports saved yet.")).toBeInTheDocument());
  expect(reportAttempts).toBe(2);
});

/**
 * The operator's report: ten loaded worker runtimes holding 6 GiB read as
 * "Critical" on a 32 GiB machine with no memory stall at all, and nothing on
 * the page said how big the machine was. Six gigabytes means something very
 * different on 32 than on 8, and every row was being read without that.
 */
test("states the machine's size and verdict above the rows it makes sense of", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    if (String(input).includes("runtime/resources")) {
      return new Response(JSON.stringify({
        sampled_at: 0,
        policy: { mode: "observe_only", advisory_percent: 15, critical_percent: 25 },
        api: { resident_memory_bytes: 100, process_tree_resident_memory_bytes: 100, process_tree_process_count: 1, pressure: "normal" },
        terminal_host: { resident_memory_bytes: 100, process_tree_resident_memory_bytes: 100, process_tree_process_count: 1, pressure: "normal" },
        machine: {
          memory_total_bytes: 32 * 1024 * 1024 * 1024,
          memory_available_bytes: null, memory_used_percent: 45, swap_total_bytes: null,
          swap_used_bytes: null, swap_used_percent: null, load_average: [1.2, 1.0, 0.9],
          logical_cpus: 8, memory_pressure_avg10: 0, cpu_pressure_avg10: 0,
          io_pressure_avg10: 0, pressure: "normal",
        },
      }), { status: 200, headers: { "Content-Type": "application/json" } });
    }
    return new Response("unavailable", { status: 503 });
  }));

  render(<DiagnosticsWorkspace feedbackRevision={0} operatorToken="secret" health={undefined} hiveIdentity={undefined} liveFeedState="connected" recentEvents={[]} sessions={[]} workers={[]} jiraReadiness={undefined} jiraUnavailable={true} />);

  const headline = await screen.findByText(/of memory ·/);
  expect(headline).toHaveTextContent("32.0 GiB of memory");
  expect(headline).toHaveTextContent("8 CPUs");
  expect(headline).toHaveTextContent("not under pressure");
  // A machine that is not stalling must not have its layers called critical.
  expect(headline.className).toContain("normal");
});

test("answers the heading before showing the evidence for it", async () => {
  // Fourteen rows of equal weight under "Know which layer needs attention" did
  // not answer it. What is wrong leads; what is fine collapses behind a count.
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    if (String(input).includes("runtime/resources")) {
      return new Response(JSON.stringify({
        sampled_at: 0,
        policy: { mode: "observe_only", advisory_percent: 15, critical_percent: 25 },
        api: { resident_memory_bytes: 100, process_tree_resident_memory_bytes: 100, process_tree_process_count: 1, pressure: "normal" },
        terminal_host: { resident_memory_bytes: 100, process_tree_resident_memory_bytes: 100, process_tree_process_count: 1, pressure: "normal" },
        machine: {
          memory_total_bytes: 32 * 1024 * 1024 * 1024, memory_available_bytes: null, memory_used_percent: 45,
          swap_total_bytes: null, swap_used_bytes: null, swap_used_percent: null, load_average: [1, 1, 1],
          logical_cpus: 8, memory_pressure_avg10: 0, cpu_pressure_avg10: 0, io_pressure_avg10: 0, pressure: "normal",
        },
      }), { status: 200, headers: { "Content-Type": "application/json" } });
    }
    return new Response("unavailable", { status: 503 });
  }));

  render(<DiagnosticsWorkspace feedbackRevision={0} operatorToken="secret" health={{ status: "ok", version: "0.1.0" } as never} hiveIdentity={{ hive: { id: "h", name: "Grand Garden" } } as never} liveFeedState="connected" recentEvents={[]} sessions={[]} workers={[]} jiraReadiness={undefined} jiraUnavailable={false} />);

  // Healthy rows are not the page's answer, so they are not the page's content.
  const showAll = await screen.findByRole("button", { name: /Show all \d+ checks/ });
  expect(screen.queryByText("Machine memory")).not.toBeInTheDocument();

  fireEvent.click(showAll);
  expect(screen.getByText("Machine memory")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Show only what needs attention" })).toBeInTheDocument();
});
