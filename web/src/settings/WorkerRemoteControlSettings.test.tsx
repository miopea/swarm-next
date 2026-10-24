import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import WorkerRemoteControlSettings from "./WorkerRemoteControlSettings";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const ok = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });

/** ADR 0122: off as a Hive starts, and turning it on says what it costs. */
test("turning Remote Control on says Swarm can no longer update those workers", async () => {
  const fetch = vi.fn().mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(ok(
    init?.method === "PUT" ? { enabled: JSON.parse(String(init.body)).enabled } : { enabled: false },
  )));
  vi.stubGlobal("fetch", fetch);
  render(<WorkerRemoteControlSettings operatorToken="secret" />);
  const toggle = screen.getByRole("checkbox", { name: /Let Remote Control connect to workers/ });
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(toggle).not.toBeChecked();
  expect(screen.getByText(/Swarm moves each one onto a new Claude version by itself/)).toBeInTheDocument();

  fireEvent.click(toggle);
  expect(await screen.findByRole("status")).toHaveTextContent("Claude will not restart it, so Swarm cannot move it onto a new Claude version");
  expect(fetch.mock.calls[1]?.[1].body).toBe(JSON.stringify({ enabled: true }));
  expect(toggle).toBeChecked();
});

test("a failed save says so and leaves the setting as it was", async () => {
  vi.stubGlobal("fetch", vi.fn().mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(
    init?.method === "PUT" ? new Response("{}", { status: 500 }) : ok({ enabled: false }),
  )));
  render(<WorkerRemoteControlSettings operatorToken="secret" />);
  const toggle = screen.getByRole("checkbox", { name: /Let Remote Control connect to workers/ });
  await waitFor(() => expect(toggle).toBeEnabled());

  fireEvent.click(toggle);
  expect(await screen.findByRole("alert")).toHaveTextContent("was not saved");
  expect(toggle).not.toBeChecked();
});
