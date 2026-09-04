import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import DiagnosticsWorkspace from "./DiagnosticsWorkspace";
import { workerTreePressure } from "./DiagnosticsWorkspace";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("bounds diagnostic requests, aborts on unmount, and recovers after timeout", async () => {
  vi.useFakeTimers();
  const signals: AbortSignal[] = [];
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    if (String(input).includes("feedback/reports")) return Promise.resolve(new Response("[]"));
    signals.push(init!.signal as AbortSignal);
    return new Promise<Response>((_resolve, reject) => {
      init!.signal!.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")), { once: true });
    });
  }));
  const view = render(<DiagnosticsWorkspace feedbackRevision={0} operatorToken="secret" health={undefined} hiveIdentity={undefined} liveFeedState="connected" recentEvents={[]} sessions={[]} workers={[]} jiraReadiness={undefined} jiraUnavailable={true} />);
  expect(signals).toHaveLength(3);
  await act(async () => { await vi.advanceTimersByTimeAsync(8_000); });
  expect(signals.every((signal) => signal.aborted)).toBe(true);
  await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
  expect(signals).toHaveLength(6);
  view.unmount();
  expect(signals.every((signal) => signal.aborted)).toBe(true);
  await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
  expect(signals).toHaveLength(6);
});

test("hidden diagnostics do not poll and become fresh when visible", async () => {
  vi.useFakeTimers();
  let visibility: DocumentVisibilityState = "hidden";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
  const fetch = vi.fn(async (input: RequestInfo | URL) => String(input).includes("feedback/reports")
    ? new Response("[]") : new Response("unavailable", { status: 503 }));
  vi.stubGlobal("fetch", fetch);
  render(<DiagnosticsWorkspace feedbackRevision={0} operatorToken="secret" health={undefined} hiveIdentity={undefined} liveFeedState="connected" recentEvents={[]} sessions={[]} workers={[]} jiraReadiness={undefined} jiraUnavailable={true} />);
  await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
  expect(fetch).toHaveBeenCalledTimes(1); // saved reports only
  visibility = "visible";
  await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
  expect(fetch).toHaveBeenCalledTimes(4);
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
  expect(headline).toHaveTextContent("not under memory pressure");
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

/**
 * "Compute is high because I just opened a bunch of workers, but why is memory
 * critical?" Fifteen workers holding 7 GiB on a 31 GiB machine reporting no
 * memory stall were reported Critical, because this page carried its own
 * absolute thresholds — 4 GiB critical — and never looked at the machine.
 *
 * That rule already exists on the server, judged against the machine's size and
 * whether it is actually stalling. The page defers to it now.
 */
test("a large but proportionate worker footprint is not called critical", () => {
  const roomy = {
    resident_memory_bytes: 40 * 1024 * 1024,
    process_tree_resident_memory_bytes: 7 * 1024 * 1024 * 1024,
    process_tree_process_count: 15,
    // 7 GiB of 31 GiB, on a machine the kernel says is not stalling.
    pressure: "normal" as const,
  };
  expect(workerTreePressure(roomy)).toBe("normal");

  // And a genuinely strained machine still reads through.
  expect(workerTreePressure({ ...roomy, pressure: "critical" as const })).toBe("critical");
});

const diagnosticsProps = {
  feedbackRevision: 0,
  operatorToken: "secret",
  hiveIdentity: undefined,
  liveFeedState: "retrying" as const,
  recentEvents: [],
  sessions: [],
  workers: [],
  jiraReadiness: undefined,
  jiraUnavailable: true,
};

test("a subsystem disabled at startup leads the page instead of sitting unnoticed", () => {
  // THE ABLATION. Drop the degraded rows and this Hive looks entirely healthy:
  // every other check passes, because the process IS serving. That is the
  // failure mode the reporting exists for — email silently never arriving is
  // an outage nobody is told about.
  render(
    <DiagnosticsWorkspace
      {...diagnosticsProps}
      health={{
        status: "degraded",
        version: "0.8.19",
        degraded: [
          {
            subsystem: "Microsoft email",
            reason: "Microsoft email OAuth requires SWARM_PUBLIC_BASE_URL",
          },
        ],
      }}
    />,
  );
  expect(screen.getByText("Microsoft email configuration")).toBeInTheDocument();
  expect(
    screen.getByText(/Disabled at startup · Microsoft email OAuth requires SWARM_PUBLIC_BASE_URL/),
  ).toBeInTheDocument();
});

test("a Hive with nothing switched off shows no disabled rows", () => {
  render(
    <DiagnosticsWorkspace
      {...diagnosticsProps}
      health={{ status: "ok", version: "0.8.19", degraded: [] }}
    />,
  );
  expect(screen.queryByText(/Disabled at startup/)).not.toBeInTheDocument();
});
