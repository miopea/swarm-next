import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import QueenReleasesSettings from "./QueenReleasesSettings";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const ok = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });

/** ADR 0131: off as a Hive starts, and turning it on says what it still leaves to the operator. */
test("turning Queen releases on says what the operator is still asked about", async () => {
  const fetch = vi.fn().mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(ok(
    init?.method === "PUT" ? { enabled: JSON.parse(String(init.body)).enabled } : { enabled: false },
  )));
  vi.stubGlobal("fetch", fetch);
  render(<QueenReleasesSettings operatorToken="secret" />);
  const toggle = screen.getByRole("checkbox", { name: /Queen releases reviewed work/ });
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(toggle).not.toBeChecked();
  expect(screen.getByText(/you are asked to approve it/)).toBeInTheDocument();

  fireEvent.click(toggle);
  expect(await screen.findByRole("status")).toHaveTextContent("still asked about production data, credentials, permissions, admin merges and failed checks");
  expect(String(fetch.mock.calls[1]?.[0])).toContain("/api/v1/runtime/queen-releases");
  expect(fetch.mock.calls[1]?.[1].body).toBe(JSON.stringify({ enabled: true }));
  expect(toggle).toBeChecked();
});

test("a failed save says so and leaves Queen's release authority as it was", async () => {
  vi.stubGlobal("fetch", vi.fn().mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(
    init?.method === "PUT" ? new Response("{}", { status: 401 }) : ok({ enabled: false }),
  )));
  render(<QueenReleasesSettings operatorToken="secret" />);
  const toggle = screen.getByRole("checkbox", { name: /Queen releases reviewed work/ });
  await waitFor(() => expect(toggle).toBeEnabled());

  fireEvent.click(toggle);
  expect(await screen.findByRole("alert")).toHaveTextContent("was not saved");
  expect(toggle).not.toBeChecked();
});
