import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import TaskReadsSettings from "./TaskReadsSettings";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const ok = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });

/** ADR 0146: off as a Hive starts, and turning it on says what still comes to the operator. */
test("turning task reads on says what the operator is still asked about", async () => {
  const fetch = vi.fn().mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(ok(
    init?.method === "PUT" ? { enabled: JSON.parse(String(init.body)).enabled } : { enabled: false },
  )));
  vi.stubGlobal("fetch", fetch);
  render(<TaskReadsSettings operatorToken="secret" />);
  const toggle = screen.getByRole("checkbox", { name: /Workers read production for their task/ });
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(toggle).not.toBeChecked();
  expect(screen.getByText(/you are asked to allow it/)).toBeInTheDocument();

  fireEvent.click(toggle);
  const status = await screen.findByRole("status");
  expect(status).toHaveTextContent("still asked about every write, setting or permission change");
  expect(status).toHaveTextContent("Deploys are not reads: the Queen releases setting decides those.");
  expect(status).toHaveTextContent("anything that spends money");
  expect(status).toHaveTextContent("any read of names, addresses or payment details");
  expect(String(fetch.mock.calls[1]?.[0])).toContain("/api/v1/runtime/task-reads");
  expect(fetch.mock.calls[1]?.[1].body).toBe(JSON.stringify({ enabled: true }));
  expect(toggle).toBeChecked();
});

test("a failed save says so and leaves task reads as they were", async () => {
  vi.stubGlobal("fetch", vi.fn().mockImplementation((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(
    init?.method === "PUT" ? new Response("{}", { status: 401 }) : ok({ enabled: false }),
  )));
  render(<TaskReadsSettings operatorToken="secret" />);
  const toggle = screen.getByRole("checkbox", { name: /Workers read production for their task/ });
  await waitFor(() => expect(toggle).toBeEnabled());

  fireEvent.click(toggle);
  expect(await screen.findByRole("alert")).toHaveTextContent("was not saved");
  expect(toggle).not.toBeChecked();
});
