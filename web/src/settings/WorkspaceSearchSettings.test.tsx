import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import WorkspaceSearchSettings from "./WorkspaceSearchSettings";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
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
