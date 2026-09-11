import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import WorkspaceSearchSettings from "./WorkspaceSearchSettings";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.useRealTimers(); });
const initial = { revision: 1, folders: ["/projects/work"], installation_folders: ["/installed"], health: [{ path: "/projects/work", available: false }] };
function expand() {
  const details = screen.getByText("Repository search folders").closest("details")!;
  details.open = true;
  fireEvent(details, new Event("toggle"));
}

test("loads only on opening, preserves a rejected edit and refreshes discovery after retry", async () => {
  let attempts = 0;
  const fetch = vi.fn(async (_url: unknown, init?: RequestInit) => {
    if (init?.method !== "PUT") return Response.json(initial);
    expect(JSON.parse(String(init.body))).toEqual({ revision: 1, folders: ["~/projects"] });
    if (++attempts === 1) return Response.json({ message: "Folder unavailable" }, { status: 422 });
    return Response.json({ ...initial, revision: 2, folders: ["/home/operator/projects"], health: [] });
  });
  vi.stubGlobal("fetch", fetch);
  const onSaved = vi.fn().mockResolvedValue(undefined);
  render(<WorkspaceSearchSettings operatorToken="test" onSaved={onSaved} />);
  expect(fetch).not.toHaveBeenCalled();
  expand();
  const input = await screen.findByLabelText("Additional trusted folders");
  expect(screen.getByText(/Unavailable: \/projects\/work/)).toBeInTheDocument();
  fireEvent.change(input, { target: { value: "~/projects" } });
  fireEvent.click(screen.getByRole("button", { name: "Save search folders" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Folder unavailable");
  expect(input).toHaveValue("~/projects");
  expect(onSaved).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Save search folders" }));
  await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
  expect(input).toHaveValue("/home/operator/projects");
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("a stale editor explicitly reloads saved state instead of silently overwriting", async () => {
  let reads = 0;
  vi.stubGlobal("fetch", vi.fn(async (_url: unknown, init?: RequestInit) => init?.method === "PUT"
    ? Response.json({ message: "Search folders changed elsewhere" }, { status: 409 })
    : Response.json(++reads === 1 ? initial : { ...initial, revision: 2, folders: ["/new"] })));
  render(<WorkspaceSearchSettings operatorToken="test" onSaved={vi.fn()} />);
  expand();
  const input = await screen.findByLabelText("Additional trusted folders");
  fireEvent.change(input, { target: { value: "/my-edit" } });
  fireEvent.click(screen.getByRole("button", { name: "Save search folders" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("changed elsewhere");
  expect(input).toHaveValue("/my-edit");
  fireEvent.click(screen.getByRole("button", { name: "Reload saved folders (discard edit)" }));
  await waitFor(() => expect(screen.getByLabelText("Additional trusted folders")).toHaveValue("/new"));
});

test("saved folders recover discovery without repeating the write or discarding an edit", async () => {
  const fetch = vi.fn(async (_url: unknown, init?: RequestInit) => Response.json({ ...initial, revision: init?.method === "PUT" ? 2 : 1 }));
  vi.stubGlobal("fetch", fetch);
  const onSaved = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValue(undefined);
  render(<WorkspaceSearchSettings operatorToken="test" onSaved={onSaved} />);
  expand();
  const input = await screen.findByLabelText("Additional trusted folders");
  fireEvent.click(screen.getByRole("button", { name: "Save search folders" }));
  const retry = await screen.findByRole("button", { name: "Refresh repository list" });
  expect(screen.getByRole("alert")).toHaveTextContent("You do not need to save again");
  fireEvent.change(input, { target: { value: "/another-edit" } });
  fireEvent.click(retry);
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  expect(input).toHaveValue("/another-edit");
  expect(fetch.mock.calls.filter(([, init]) => init?.method === "PUT")).toHaveLength(1);
  expect(onSaved).toHaveBeenCalledTimes(2);
});

test("a stalled folder load times out and can retry", async () => {
  vi.useFakeTimers();
  let first = true;
  let signal: AbortSignal | null | undefined;
  vi.stubGlobal("fetch", vi.fn(async (_url: unknown, init?: RequestInit) => {
    if (!first) return Response.json(initial);
    first = false;
    signal = init?.signal;
    return new Promise<Response>((_resolve, reject) => signal?.addEventListener("abort", () => reject(new DOMException("Cancelled", "AbortError"))));
  }));
  render(<WorkspaceSearchSettings operatorToken="test" onSaved={vi.fn()} />);
  expand();
  await act(async () => { await vi.advanceTimersByTimeAsync(8_000); });
  expect(signal?.aborted).toBe(true);
  expect(screen.getByRole("alert")).toHaveTextContent("took too long");
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Retry loading folders" })); });
  expect(screen.getByLabelText("Additional trusted folders")).toHaveValue("/projects/work");
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});
