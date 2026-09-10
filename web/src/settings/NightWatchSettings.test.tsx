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

test("a rejected time zone explains the correction instead of asking for the same retry", async () => {
  const fetch = vi.fn().mockResolvedValueOnce(ok(config)).mockResolvedValueOnce(new Response(JSON.stringify({ message: "invalid schedule" }), { status: 400 }));
  vi.stubGlobal("fetch", fetch);
  render(<NightWatchSettings operatorToken="secret" />);
  await waitFor(() => expect(screen.getByRole("button", { name: "Save schedule" })).toBeEnabled());
  fireEvent.change(screen.getByLabelText("Time zone"), { target: { value: "New York" } });
  fireEvent.click(screen.getByRole("button", { name: "Save schedule" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Schedule not saved. Use a region/city time zone");
  expect(screen.getByRole("alert")).not.toHaveTextContent("not confirmed");
  expect(screen.getByLabelText("Time zone")).toHaveValue("New York");
  expect(screen.getByLabelText("Saved Night Watch schedule")).toHaveTextContent("America/New_York");
});

test("draft schedule does not replace the saved schedule until confirmed", async () => {
  const updated = { ...config, start_minute: 1380 };
  const fetch = vi.fn().mockResolvedValueOnce(ok(config)).mockResolvedValueOnce(ok(updated));
  vi.stubGlobal("fetch", fetch);
  render(<NightWatchSettings operatorToken="secret" />);
  await waitFor(() => expect(screen.getByRole("button", { name: "Save schedule" })).toBeEnabled());
  expect(screen.getByLabelText("Saved Night Watch schedule")).toHaveTextContent("22:00–07:00 the next day");
  fireEvent.change(screen.getByLabelText("Starts"), { target: { value: "23:00" } });
  expect(screen.getByText("Unsaved schedule changes.")).toBeInTheDocument();
  expect(screen.getByLabelText("Saved Night Watch schedule")).toHaveTextContent("22:00–07:00");
  fireEvent.click(screen.getByRole("button", { name: "Save schedule" }));
  await screen.findByText("Schedule saved.");
  expect(screen.getByLabelText("Saved Night Watch schedule")).toHaveTextContent("23:00–07:00");
  expect(screen.queryByText("Unsaved schedule changes.")).not.toBeInTheDocument();
});

test("same-day and disabled schedules do not claim overnight automation", async () => {
  const fetch = vi.fn().mockResolvedValueOnce(ok({ ...config, start_minute: 360, end_minute: 480 })).mockResolvedValueOnce(ok({ ...config, enabled: false }));
  vi.stubGlobal("fetch", fetch);
  render(<NightWatchSettings operatorToken="secret" />);
  await waitFor(() => expect(screen.getByRole("button", { name: "Save schedule" })).toBeEnabled());
  expect(screen.getByLabelText("Saved Night Watch schedule")).toHaveTextContent("06:00–08:00");
  expect(screen.getByLabelText("Saved Night Watch schedule")).not.toHaveTextContent("next day");
  fireEvent.click(screen.getByLabelText("Enable scheduled Night Watch"));
  fireEvent.click(screen.getByRole("button", { name: "Save schedule" }));
  await screen.findByText("Schedule saved.");
  expect(screen.getByLabelText("Saved Night Watch schedule")).toHaveTextContent("No automatic Night Watch is scheduled");
});

test("a first schedule's time-zone edit is unsaved and does not enable automation", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(ok(null)));
  render(<NightWatchSettings operatorToken="secret" />);
  await waitFor(() => expect(screen.getByRole("button", { name: "Save schedule" })).toBeEnabled());
  fireEvent.change(screen.getByLabelText("Time zone"), { target: { value: "Pacific/Auckland" } });
  expect(screen.getByText("Unsaved schedule changes.")).toBeInTheDocument();
  expect(screen.getByLabelText("Enable scheduled Night Watch")).not.toBeChecked();
  expect(screen.getByLabelText("Saved Night Watch schedule")).toHaveTextContent("No automatic Night Watch");
});
