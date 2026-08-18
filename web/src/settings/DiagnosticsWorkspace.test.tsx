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
