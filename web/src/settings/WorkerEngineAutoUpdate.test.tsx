import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";

const setWorkerEngineAutoUpdate = vi.fn();
vi.mock("../api", () => ({
  setWorkerEngineAutoUpdate: (...args: unknown[]) => setWorkerEngineAutoUpdate(...args),
}));

import WorkerEngineAutoUpdate from "./WorkerEngineAutoUpdate";


test("the card shows the switch, what the update waits for, and turns it off", async () => {
  setWorkerEngineAutoUpdate.mockResolvedValue({ enabled: false });
  render(<WorkerEngineAutoUpdate
    operatorToken="fixture-token"
    behind
    view={{ enabled: true, refusal: { reason: "turn_in_progress", worker: "Pantry", since: Math.floor(Date.now() / 1000) }, last_update_workers: [] }}
  />);
  const toggle = screen.getByRole("checkbox", { name: /Update by itself when every worker is resting/ });
  expect(toggle).toBeChecked();
  expect(screen.getByRole("status")).toHaveTextContent("Not yet: Pantry is mid-turn.");

  fireEvent.click(toggle);
  await waitFor(() => expect(toggle).not.toBeChecked());
  expect(setWorkerEngineAutoUpdate).toHaveBeenCalledWith("fixture-token", false);
  expect(screen.getByRole("status")).toHaveTextContent("Automatic engine updates are off");
});

test("with nothing waiting it shows only the switch and the last update", () => {
  render(<WorkerEngineAutoUpdate
    operatorToken="fixture-token"
    behind={false}
    view={{ enabled: true, refusal: null, last_update_workers: [] }}
    history="Swarm updated the worker engine by itself moments ago."
  />);
  expect(screen.getByRole("checkbox")).toBeChecked();
  expect(screen.getByRole("status")).toHaveTextContent("Swarm updated the worker engine by itself moments ago.");
  expect(screen.queryByText(/Not yet/)).toBeNull();
});

test("a failed save keeps the switch where it was and says so", async () => {
  setWorkerEngineAutoUpdate.mockImplementation(async () => { throw new Error("offline"); });
  render(<WorkerEngineAutoUpdate operatorToken="fixture-token" behind={false} view={{ enabled: true, refusal: null, last_update_workers: [] }} />);
  fireEvent.click(screen.getByRole("checkbox"));
  expect(await screen.findByRole("alert")).toHaveTextContent("The setting was not saved");
  expect(screen.getByRole("checkbox")).toBeChecked();
});
