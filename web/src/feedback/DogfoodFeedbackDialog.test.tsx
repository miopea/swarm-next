import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import DogfoodFeedbackDialog, { destinationLabel } from "./DogfoodFeedbackDialog";

afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

test("editing feedback does not multiply readiness requests", async () => {
  stubDialogFetch({ configured: false, repository: null });
  renderDialog();
  await screen.findByText(/cannot file to GitHub/);
  const fetch = vi.mocked(globalThis.fetch);
  const reads = () => fetch.mock.calls.filter(([input]) => /feedback\/github$|integrations\/github\/connection$/.test(String(input))).length;
  expect(reads()).toBe(2);
  for (let index = 1; index <= 20; index += 1) {
    fireEvent.change(screen.getByLabelText("What did you expect?"), { target: { value: "a".repeat(index) } });
    fireEvent.change(screen.getByLabelText("What happened instead?"), { target: { value: "b".repeat(index) } });
  }
  expect(reads()).toBe(2);
});

test.each(["close", "deadline"])("feedback evidence reads abort on %s", async (reason) => {
  vi.useFakeTimers();
  const signals: AbortSignal[] = [];
  vi.stubGlobal("fetch", vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
    const signal = init!.signal!;
    signals.push(signal);
    return new Promise<Response>((_resolve, reject) => signal.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true }));
  }));
  const view = renderDialog();
  expect(signals).toHaveLength(5);
  if (reason === "close") {
    view.unmount();
    await act(async () => {});
  } else {
    await act(async () => { await vi.advanceTimersByTimeAsync(8_000); });
    expect(screen.getByRole("button", { name: "Preview bundle" })).toBeEnabled();
    expect(screen.getByText(/cannot file to GitHub/)).toBeInTheDocument();
  }
  expect(signals.every((signal) => signal.aborted)).toBe(true);
});

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
  expect(onClose).not.toHaveBeenCalled();
  expect(screen.getByRole("alertdialog", { name: "Discard this feedback?" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Keep editing" })).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "Discard feedback" }));
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
  const onSaved = vi.fn();
  render(<DogfoodFeedbackDialog activeSessionId={undefined} health={{ status: "ok", version: "0.1.0" }} hiveIdentity={undefined} liveFeedState="connected" onClose={vi.fn()} onSaved={onSaved} operatorToken="token" recentEvents={[]} sessions={[]} surface="workers" workers={[]} />);
  fireEvent.change(screen.getByLabelText("What did you expect?"), { target: { value: "Worker should recover" } });
  fireEvent.change(screen.getByLabelText("What happened instead?"), { target: { value: "Terminal stayed blank" } });
  const image = new File([new Uint8Array([1, 2, 3])], "terminal.png", { type: "image/png" });
  fireEvent.paste(screen.getByRole("dialog"), { clipboardData: { files: [image] } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Save to this Hive" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Save to this Hive" }));

  expect(await screen.findByText(/sent nowhere else/)).toBeInTheDocument();
  expect(onSaved).toHaveBeenCalledOnce();
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

/**
 * THE COMMITTING BUTTON MUST NOT NAME A DESTINATION IT HAS NOT ESTABLISHED.
 *
 * `github` is fetched asynchronously and starts undefined, so `github?.configured`
 * read falsy during the fetch and the button rendered "Save to this Hive" — then
 * swapped itself to "Send to GitHub" when the response landed. The operator
 * watched that happen while typing and reasonably concluded the typing caused it.
 *
 * The two labels are not two wordings for one act. One keeps the words on this
 * machine; the other publishes them to a public issue tracker under the
 * operator's own account.
 */
test("says it is still checking rather than claiming a destination it does not know", () => {
  expect(destinationLabel(undefined, "idle")).toBe("Checking where this goes…");
  expect(destinationLabel(undefined, "idle")).not.toMatch(/Hive|GitHub/);
});

test("names the destination once it is known, and only then", () => {
  expect(destinationLabel({ configured: false }, "idle")).toBe("Save to this Hive");
  expect(destinationLabel({ configured: true }, "idle")).toBe("Send to GitHub");
  expect(destinationLabel({ configured: true }, "saving")).toBe("Sending…");
  expect(destinationLabel({ configured: false }, "saved")).toBe("Saved to Hive");
});

/**
 * ONE PRIMARY. The operator's words were "three buttons and none are clear",
 * and that was literally true of the markup: two of the three carried
 * `primary-action`, so the dialog nominated two winners.
 */
test("exactly one action in the row reads as the primary one", () => {
  const { container } = render(<DogfoodFeedbackDialog activeSessionId={undefined} health={{ status: "ok", version: "0.1.0" }} hiveIdentity={undefined} liveFeedState="connected" onClose={vi.fn()} operatorToken="token" recentEvents={[]} sessions={[]} surface="workers" workers={[]} />);
  // THE WHOLE DIALOG, not just the action row. The claim is that one control
  // reads as primary; scoping the query to the row would leave a second primary
  // anywhere else in the dialog — including the connect panel it renders —
  // passing a test whose sentence says otherwise.
  expect(container.querySelectorAll(".primary-action")).toHaveLength(1);
  // And it is the one that finishes the job, not one of the two that precede it.
  expect(container.querySelector(".primary-action")!.textContent).toMatch(/Hive|GitHub|Checking/);
  const row = container.querySelector(".diagnostic-actions");
  expect(row).not.toBeNull();
  expect(row!.contains(container.querySelector(".primary-action"))).toBe(true);
});

/** Answers every call the dialog makes on open, with a chosen readiness. */
function stubDialogFetch(github: { configured: boolean; repository: string | null }) {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("feedback/github/connection")) return ok({ connected: false, lapsed: false, login: null });
    if (url.includes("feedback/github")) return ok(github);
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 0, retained_sessions: 0 } });
    if (url.includes("runtime/resources")) return ok({
      sampled_at: 1,
      policy: { mode: "observe_only", advisory_bytes: 268_435_456, critical_bytes: 536_870_912 },
      api: { resident_memory_bytes: 1, pressure: "normal" },
      terminal_host: { resident_memory_bytes: null, pressure: "unavailable" },
    });
    return ok({ type: "history_diagnostics", diagnostics: { retained_bytes: 0, session_count: 0, segment_count: 0, dropped_records: 0, dropped_bytes: 0, recovered_truncated_bytes: 0, recovered_corrupt_segments: 0 } });
  }));
}

function renderDialog() {
  return render(<DogfoodFeedbackDialog activeSessionId={undefined} health={{ status: "ok", version: "0.1.0" }} hiveIdentity={undefined} liveFeedState="connected" onClose={vi.fn()} operatorToken="token" recentEvents={[]} sessions={[]} surface="workers" workers={[]} />);
}

/**
 * THE SILENT DEGRADATION, WHICH IS A SEPARATE DEFECT FROM WHERE THE CREDENTIAL
 * COMES FROM.
 *
 * "Save to this Hive" is a true description of what happens and reads as a
 * CHOICE — an install deliberately keeping reports local. An install that
 * cannot reach the project at all shows exactly the same words, and the two are
 * indistinguishable on screen. That is why this survived a release: a fresh
 * Hive looked configured-for-local rather than unable-to-file.
 */
test("a Hive that cannot file says so, rather than only offering to save locally", async () => {
  stubDialogFetch({ configured: false, repository: null });
  renderDialog();
  await waitFor(() => expect(screen.getByText(/cannot file to GitHub/)).toBeTruthy());
  // And it says what follows from that, in the reporter's terms: their words
  // stop here and nobody upstream sees them.
  expect(screen.getByText(/maintainers will not see it/)).toBeTruthy();
});

/** The notice is about inability, so a Hive that CAN file must not show it. */
test("a Hive that can file shows no cannot-file notice", async () => {
  stubDialogFetch({ configured: true, repository: "miopea/swarm-next" });
  renderDialog();
  await waitFor(() => expect(screen.getByRole("button", { name: "Send to GitHub" })).toBeTruthy());
  expect(screen.queryByText(/cannot file to GitHub/)).toBeNull();
});
