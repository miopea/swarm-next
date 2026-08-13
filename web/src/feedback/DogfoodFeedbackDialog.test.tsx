import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import DogfoodFeedbackDialog from "./DogfoodFeedbackDialog";

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

test("previews an explicit dogfood note with content-free runtime context", async () => {
  const onClose = vi.fn();
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 1, retained_sessions: 1 } });
    if (url.includes("runtime/resources")) return ok({
      sampled_at: 1,
      policy: { mode: "observe_only", advisory_bytes: 268_435_456, critical_bytes: 536_870_912 },
      api: { resident_memory_bytes: 18_874_368, pressure: "normal" },
      terminal_host: { resident_memory_bytes: null, pressure: "unavailable" },
    });
    return ok({ type: "history_diagnostics", diagnostics: { retained_bytes: 42, session_count: 1, segment_count: 1, dropped_records: 0, dropped_bytes: 0, recovered_truncated_bytes: 0, recovered_corrupt_segments: 0 } });
  }));

  render(
    <DogfoodFeedbackDialog
      activeSessionId="selected-session-id"
      health={{ status: "ok", version: "0.1.0" }}
      hiveIdentity={{ operator: { id: "operator-1", display_name: "Bea" }, hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: null } }}
      liveFeedState="connected"
      onClose={onClose}
      operatorToken="secret-token"
      recentEvents={[{ sequence: 7, hive_id: "hive-1", kind: "workers_changed", occurred_at: 1 }]}
      sessions={[{ session_id: "selected-session-id", running: true }]}
      surface="workers"
      workers={[{ id: "worker-1", hive_id: "hive-1", name: "Private worker", role: "worker", provider: "claude_code", workspace: "/private/workspace", autostart: false, position: 1, active_session_id: "selected-session-id", created_at: 1, updated_at: 1, running: true, attention_state: "blocked", runtime_error: "raw provider failure" }]}
    />,
  );

  fireEvent.change(screen.getByLabelText("What did you expect?"), { target: { value: "The worker should stay visible." } });
  fireEvent.change(screen.getByLabelText("What happened instead?"), { target: { value: "The terminal became blank." } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Preview bundle" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Preview bundle" }));

  const preview = screen.getByLabelText("Dogfood feedback bundle");
  expect(preview).toHaveTextContent("The worker should stay visible.");
  expect(preview).toHaveTextContent("The terminal became blank.");
  expect(preview).toHaveTextContent("selected-session-id");
  expect(preview).toHaveTextContent('"surface": "workers"');
  expect(preview).toHaveTextContent("18874368");
  expect(preview).not.toHaveTextContent("Private worker");
  expect(preview).not.toHaveTextContent("/private/workspace");
  expect(preview).not.toHaveTextContent("raw provider failure");
  expect(preview).not.toHaveTextContent("secret-token");
  fireEvent.keyDown(window, { key: "Escape" });
  expect(onClose).toHaveBeenCalledOnce();
});

test("attaches a pasted screenshot locally and records only safe metadata", async () => {
  vi.stubGlobal("URL", { ...URL, createObjectURL: vi.fn().mockReturnValue("blob:screenshot"), revokeObjectURL: vi.fn() });
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 0, retained_sessions: 0 } });
    if (url.includes("runtime/resources")) return ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 1, critical_bytes: 2 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" } });
    return ok({ type: "history_diagnostics", diagnostics: null });
  }));
  render(<DogfoodFeedbackDialog activeSessionId={undefined} health={{ status: "ok", version: "0.1.0" }} hiveIdentity={undefined} liveFeedState="connected" onClose={vi.fn()} operatorToken="token" recentEvents={[]} sessions={[]} surface="workers" workers={[]} />);
  const image = new File([new Uint8Array([1, 2, 3])], "terminal.png", { type: "image/png" });
  fireEvent.paste(screen.getByRole("dialog"), { clipboardData: { files: [image] } });

  expect(await screen.findByAltText("Attached dogfood screenshot")).toHaveAttribute("src", "blob:screenshot");
  await waitFor(() => expect(screen.getByRole("button", { name: "Preview bundle" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Preview bundle" }));
  const preview = screen.getByLabelText("Dogfood feedback bundle");
  expect(preview).toHaveTextContent("terminal.png");
  expect(preview).toHaveTextContent("image/png");
  expect(preview).toHaveTextContent("stays on this device unless the operator explicitly saves");
  expect(preview).not.toHaveTextContent("AQID");
});

test("saves reviewed notes and an optional screenshot privately to the Hive", async () => {
  vi.stubGlobal("URL", { ...URL, createObjectURL: vi.fn().mockReturnValue("blob:screenshot"), revokeObjectURL: vi.fn() });
  const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 0, retained_sessions: 0 } });
    if (url.includes("runtime/resources")) return ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 1, critical_bytes: 2 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" } });
    if (url === "/api/v1/feedback/attachments") return ok({ name: "content-hash.png" });
    if (url === "/api/v1/feedback/reports") {
      const payload = JSON.parse(String(init?.body));
      return ok({ id: "report-1", created_at: 1, ...payload });
    }
    return ok({ type: "history_diagnostics", diagnostics: null });
  });
  vi.stubGlobal("fetch", fetch);
  render(<DogfoodFeedbackDialog activeSessionId={undefined} health={{ status: "ok", version: "0.1.0" }} hiveIdentity={undefined} liveFeedState="connected" onClose={vi.fn()} operatorToken="token" recentEvents={[]} sessions={[]} surface="workers" workers={[]} />);
  fireEvent.change(screen.getByLabelText("What did you expect?"), { target: { value: "Worker should recover" } });
  fireEvent.change(screen.getByLabelText("What happened instead?"), { target: { value: "Terminal stayed blank" } });
  const image = new File([new Uint8Array([1, 2, 3])], "terminal.png", { type: "image/png" });
  fireEvent.paste(screen.getByRole("dialog"), { clipboardData: { files: [image] } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Save to this Hive" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Save to this Hive" }));

  expect(await screen.findByText(/Saved privately/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Saved to Hive" })).toBeDisabled();
  expect(fetch).toHaveBeenCalledWith("/api/v1/feedback/attachments", expect.objectContaining({ method: "POST", body: image }));
  expect(fetch).toHaveBeenCalledWith("/api/v1/feedback/reports", expect.objectContaining({
    method: "POST",
    body: expect.stringContaining('"attachment_name":"content-hash.png"'),
  }));
  fireEvent.change(screen.getByLabelText("What happened instead?"), { target: { value: "Terminal stayed blank after reload" } });
  expect(screen.getByRole("button", { name: "Save to this Hive" })).toBeEnabled();
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
