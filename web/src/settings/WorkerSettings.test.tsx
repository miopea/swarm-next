import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import type { Worker } from "../api";
import WorkerSettings from "./WorkerSettings";

const queen = worker("queen", "Queen", "/projects/queen", 0, "queen");
const budget = worker("budget", "Daisy", "/projects/budgetbug", 1);
const studio = worker("studio", "Poppy", "/projects/sculpt-studio", 2);

test("configures and reorders durable workers without exposing path entry", async () => {
  const onCreate = vi.fn().mockResolvedValue(undefined);
  const onUpdate = vi.fn().mockResolvedValue(undefined);
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
      onUpdate={onUpdate}
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

  fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
  const editForm = screen.getByRole("form", { name: "Edit Daisy" });
  fireEvent.change(within(editForm).getByLabelText("Worker name"), { target: { value: "Marigold" } });
  fireEvent.click(within(editForm).getByLabelText("Keep this worker active automatically"));
  fireEvent.click(within(editForm).getByRole("button", { name: "Save" }));
  expect(onUpdate).toHaveBeenCalledWith(budget.id, "Marigold", true);
});

test("desktop drag ordering keeps accessible arrow controls as a fallback", () => {
  const onReorder = vi.fn().mockResolvedValue(undefined);
  const { container } = render(
    <WorkerSettings
      workers={[queen, budget, studio]}
      workspaces={[]}
      busy={false}
      onCreate={vi.fn()}
      onUpdate={vi.fn()}
      onReorder={onReorder}
    />,
  );
  const rendered = within(container);
  const source = rendered.getByRole("button", { name: "Move Poppy earlier" }).closest(".configured-worker")!;
  const target = rendered.getByRole("button", { name: "Move Daisy later" }).closest(".configured-worker")!;
  const dataTransfer = { effectAllowed: "", setData: vi.fn() };
  fireEvent.dragStart(source, { dataTransfer });
  fireEvent.dragOver(target, { dataTransfer });
  fireEvent.drop(target, { dataTransfer });
  expect(onReorder).toHaveBeenCalledWith([studio.id, budget.id]);
  expect(rendered.getByRole("button", { name: "Move Poppy earlier" })).toBeInTheDocument();
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
