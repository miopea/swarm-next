import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import type { Worker } from "../api";
import WorkerSettings from "./WorkerSettings";

const queen = worker("queen", "Queen", "/projects/queen", 0, "queen");
const budget = worker("budget", "Daisy", "/projects/budgetbug", 1);
const studio = worker("studio", "Poppy", "/projects/sculpt-studio", 2);

test("configures and reorders durable workers without exposing path entry", async () => {
  const onCreate = vi.fn().mockResolvedValue(undefined);
  const onReorder = vi.fn().mockResolvedValue(undefined);
  render(
    <WorkerSettings
      workers={[queen, budget, studio]}
      workspaces={[
        { name: "budgetbug", path: budget.workspace, kind: "repository", configured_worker_id: budget.id },
        { name: "sculpt-studio", path: studio.workspace, kind: "repository", configured_worker_id: studio.id },
        { name: "public-website", path: "/projects/public-website", kind: "repository", configured_worker_id: null },
      ]}
      busy={false}
      onCreate={onCreate}
      onReorder={onReorder}
    />,
  );

  expect(screen.getByText("Pinned · always active")).toBeInTheDocument();
  expect(screen.queryByLabelText(/path/i)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Move Poppy earlier" }));
  expect(onReorder).toHaveBeenCalledWith([studio.id, budget.id]);

  fireEvent.change(screen.getByLabelText("Worker name"), { target: { value: "Clover" } });
  fireEvent.change(screen.getByLabelText("Repository"), { target: { value: "/projects/public-website" } });
  fireEvent.click(screen.getByRole("button", { name: "Add sleeping worker" }));
  expect(onCreate).toHaveBeenCalledWith("Clover", "/projects/public-website");
});

function worker(id: string, name: string, workspace: string, position: number, role: Worker["role"] = "worker"): Worker {
  return {
    id,
    hive_id: "hive",
    name,
    role,
    provider: "claude_code",
    workspace,
    autostart: role === "queen",
    position,
    active_session_id: null,
    created_at: 1,
    updated_at: 1,
    running: false,
    attention_state: "sleeping",
  };
}
