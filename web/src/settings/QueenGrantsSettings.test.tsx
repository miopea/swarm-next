import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import QueenGrantsSettings from "./QueenGrantsSettings";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const ok = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });

/** ADR 0158: off as a Hive starts, and turning it on says what still comes to the operator. */
test("turning Queen grants on says which cards still come to the operator", async () => {
  const fetch = vi.fn().mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(ok(
    init?.method === "PUT" ? { enabled: JSON.parse(String(init.body)).enabled } : { enabled: false },
  )));
  vi.stubGlobal("fetch", fetch);
  render(<QueenGrantsSettings operatorToken="secret" />);
  const toggle = screen.getByRole("checkbox", { name: /Queen grants command cards/ });
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(toggle).not.toBeChecked();
  expect(screen.getByText("Off: every command card comes to you.")).toBeInTheDocument();

  fireEvent.click(toggle);
  const status = await screen.findByRole("status");
  expect(status).toHaveTextContent("only for work she has reviewed");
  expect(status).toHaveTextContent("touches credentials or permissions");
  expect(status).toHaveTextContent("may cost more than $25");
  expect(status).toHaveTextContent("deletes something that cannot be restored");
  expect(status).toHaveTextContent("you can still answer it yourself");
  expect(String(fetch.mock.calls[1]?.[0])).toContain("/api/v1/runtime/queen-grants");
  expect(fetch.mock.calls[1]?.[1].body).toBe(JSON.stringify({ enabled: true }));
  expect(toggle).toBeChecked();
});

test("a failed save says so and leaves Queen grants as they were", async () => {
  vi.stubGlobal("fetch", vi.fn().mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(
    init?.method === "PUT" ? new Response("{}", { status: 401 }) : ok({ enabled: false }),
  )));
  render(<QueenGrantsSettings operatorToken="secret" />);
  const toggle = screen.getByRole("checkbox", { name: /Queen grants command cards/ });
  await waitFor(() => expect(toggle).toBeEnabled());

  fireEvent.click(toggle);
  expect(await screen.findByRole("alert")).toHaveTextContent("was not saved");
  expect(toggle).not.toBeChecked();
});
