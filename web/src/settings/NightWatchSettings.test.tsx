import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import NightWatchSettings from "./NightWatchSettings";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const config = { enabled: true, timezone: "America/New_York", start_minute: 1320, end_minute: 420 };
const ok = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });

test("loads local times and saves an explicit disabled schedule", async () => {
  const fetch = vi.fn().mockImplementation(() => Promise.resolve(ok(config)));
  vi.stubGlobal("fetch", fetch);
  render(<NightWatchSettings operatorToken="secret" />);
  await waitFor(() => expect(screen.getByRole("button", { name: "Save schedule" })).toBeEnabled());
  expect(screen.getByLabelText("Starts")).toHaveValue("22:00");
  expect(screen.getByLabelText("Ends")).toHaveValue("07:00");
  fireEvent.click(screen.getByLabelText("Enable scheduled Night Watch"));
  fireEvent.click(screen.getByRole("button", { name: "Save schedule" }));
  expect(await screen.findByText("Schedule saved.")).toBeInTheDocument();
  expect(fetch.mock.calls[1]?.[1].body).toBe(JSON.stringify({ ...config, enabled: false }));
});

test("failed load prevents overwriting unknown settings and can recover", async () => {
  const fetch = vi.fn().mockRejectedValueOnce(new Error("offline")).mockImplementation(() => Promise.resolve(ok(null)));
  vi.stubGlobal("fetch", fetch);
  render(<NightWatchSettings operatorToken="secret" />);
  fireEvent.click(await screen.findByRole("button", { name: "Retry schedule load" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Save schedule" })).toBeEnabled());
  expect(screen.getByLabelText("Enable scheduled Night Watch")).not.toBeChecked();
});

test("failed save retains edits and a retry confirms them", async () => {
  const fetch = vi.fn().mockResolvedValueOnce(ok(config)).mockRejectedValueOnce(new Error("offline")).mockImplementation(() => Promise.resolve(ok(config)));
  vi.stubGlobal("fetch", fetch);
  render(<NightWatchSettings operatorToken="secret" />);
  await waitFor(() => expect(screen.getByRole("button", { name: "Save schedule" })).toBeEnabled());
  fireEvent.change(screen.getByLabelText("Starts"), { target: { value: "23:00" } });
  fireEvent.click(screen.getByRole("button", { name: "Save schedule" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("not confirmed");
  expect(screen.getByLabelText("Starts")).toHaveValue("23:00");
  fireEvent.click(screen.getByRole("button", { name: "Save schedule" }));
  expect(await screen.findByText("Schedule saved.")).toBeInTheDocument();
});
