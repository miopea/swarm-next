import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import NativeAnswerResolution from "./NativeAnswerResolution";
import * as api from "../api";

afterEach(cleanup);

test("shows the current setting rather than assuming one", async () => {
  vi.spyOn(api, "fetchNativeAnswerResolution").mockResolvedValue({ enabled: false });
  render(<NativeAnswerResolution operatorToken="token" />);

  await waitFor(() => expect(screen.getByRole("checkbox")).not.toBeChecked());
  expect(screen.getByText(/Off — answer in Swarm/)).toBeInTheDocument();
});

test("turns the switch off through the credentialled write", async () => {
  vi.spyOn(api, "fetchNativeAnswerResolution").mockResolvedValue({ enabled: true });
  const write = vi.spyOn(api, "setNativeAnswerResolution").mockResolvedValue({ enabled: false });
  render(<NativeAnswerResolution operatorToken="token" />);

  await waitFor(() => expect(screen.getByRole("checkbox")).toBeChecked());
  fireEvent.click(screen.getByRole("checkbox"));

  await waitFor(() => expect(write).toHaveBeenCalledWith("token", false));
  await waitFor(() => expect(screen.getByRole("checkbox")).not.toBeChecked());
});

/**
 * A refused write leaves the switch where it was, and says so.
 *
 * ⚠️ AN OPTIMISTIC TOGGLE WOULD BE A LIE HERE. The operator would see it flip,
 * believe terminal answers were off, and they would still be resolving
 * decisions in their name. The refusal is also expected rather than exceptional:
 * this write needs an operator credential precisely so a worker cannot make it.
 */
test("a refused change leaves the switch alone and explains why", async () => {
  vi.spyOn(api, "fetchNativeAnswerResolution").mockResolvedValue({ enabled: true });
  vi.spyOn(api, "setNativeAnswerResolution").mockRejectedValue(new Error("401"));
  render(<NativeAnswerResolution operatorToken="token" />);

  await waitFor(() => expect(screen.getByRole("checkbox")).toBeChecked());
  fireEvent.click(screen.getByRole("checkbox"));

  await waitFor(() => expect(screen.getByRole("alert")).toBeInTheDocument());
  expect(screen.getByRole("checkbox")).toBeChecked();
  expect(screen.getByText(/the setting is unchanged/)).toBeInTheDocument();
  expect(screen.getByText(/a worker on this machine cannot/)).toBeInTheDocument();
});

test("says it is still reading rather than showing a guess", () => {
  vi.spyOn(api, "fetchNativeAnswerResolution").mockReturnValue(new Promise(() => {}));
  render(<NativeAnswerResolution operatorToken="token" />);

  expect(screen.getByRole("status")).toHaveTextContent("Reading the current setting");
  expect(screen.getByRole("checkbox")).toBeDisabled();
});
