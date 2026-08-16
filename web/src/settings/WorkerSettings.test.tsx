import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { Worker } from "../api";
import WorkerSettings from "./WorkerSettings";

const queen = worker("queen", "Queen", "/projects/queen", 0, "queen");
const budget = worker("budget", "Daisy", "/projects/budgetbug", 1);
const studio = worker("studio", "Poppy", "/projects/sculpt-studio", 2);

afterEach(cleanup);

test("configures and reorders durable workers with progressive path completion", async () => {
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
      providers={{ claude_code: true, codex: false }}
      onCreate={onCreate}
      onUpdate={onUpdate}
      onRemove={vi.fn().mockResolvedValue(undefined)}
      onDraftDescription={vi.fn().mockResolvedValue("Drafted routing context.")}
      onReorder={onReorder}
    />,
  );

  expect(screen.getByText("Pinned · always active")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Move Poppy earlier" }));
  expect(onReorder).toHaveBeenCalledWith([studio.id, budget.id]);

  fireEvent.change(screen.getByLabelText("Worker name"), { target: { value: "Clover" } });
  expect(screen.getByRole("option", { name: "Codex · waiting for maintenance" })).toBeDisabled();
  const pathInput = screen.getByLabelText("Repository");
  fireEvent.focus(pathInput);
  fireEvent.change(pathInput, { target: { value: "projects/pub" } });
  expect(screen.getByRole("option", { name: /\/projects\/public-website Repository/ })).toBeInTheDocument();
  fireEvent.keyDown(pathInput, { key: "Enter" });
  expect(pathInput).toHaveValue("/projects/public-website");
  fireEvent.click(screen.getByRole("button", { name: "Add sleeping worker" }));
  expect(onCreate).toHaveBeenCalledWith("Clover", "/projects/public-website", "claude_code", false);

  fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
  const editForm = screen.getByRole("form", { name: "Edit Daisy" });
  expect(within(editForm).getByText(budget.workspace)).toBeInTheDocument();
  fireEvent.change(within(editForm).getByLabelText("Worker name"), { target: { value: "Marigold" } });
  fireEvent.change(within(editForm).getByLabelText("Queen routing description"), { target: { value: "Owns budgets and bills." } });
  fireEvent.click(within(editForm).getByLabelText("Keep this worker active automatically"));
  fireEvent.click(within(editForm).getByRole("button", { name: "Save" }));
  expect(onUpdate).toHaveBeenCalledWith(budget.id, "Marigold", "Owns budgets and bills.", "claude_code", true);
});

test("requires explicit confirmation before removing a sleeping worker", async () => {
  const onRemove = vi.fn().mockResolvedValue(undefined);
  render(
    <WorkerSettings
      workers={[queen, budget]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={vi.fn()}
      onUpdate={vi.fn()}
      onRemove={onRemove}
      onDraftDescription={vi.fn()}
      onReorder={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(screen.getByRole("button", { name: "Remove worker" }));
  expect(screen.getByText("Remove Daisy from this Hive?")).toBeInTheDocument();
  expect(onRemove).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));
  expect(onRemove).toHaveBeenCalledWith(budget.id);
});

test("accepts a complete typed path when it is not in the bounded suggestions", () => {
  const onCreate = vi.fn().mockResolvedValue(undefined);
  const { container } = render(
    <WorkerSettings
      workers={[queen]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: false }}
      onCreate={onCreate}
      onUpdate={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn()}
      onReorder={vi.fn()}
    />,
  );
  const rendered = within(container);

  fireEvent.change(rendered.getByPlaceholderText("Daisy"), { target: { value: "Clover" } });
  fireEvent.change(rendered.getByPlaceholderText("Search by name or path"), { target: { value: "/home/bschleifer/projects/personal/budgetbug" } });
  expect(rendered.getByText(/No suggestion yet/)).toBeInTheDocument();
  fireEvent.click(rendered.getByLabelText(/Use this path outside discovered project folders/));
  fireEvent.click(rendered.getByRole("button", { name: "Add sleeping worker" }));
  expect(onCreate).toHaveBeenCalledWith("Clover", "/home/bschleifer/projects/personal/budgetbug", "claude_code", true);
});

test("offers Codex only when the terminal host reports it ready", () => {
  const onCreate = vi.fn().mockResolvedValue(undefined);
  const { container } = render(
    <WorkerSettings
      workers={[queen]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={onCreate}
      onUpdate={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn()}
      onReorder={vi.fn()}
    />,
  );
  const rendered = within(container);
  fireEvent.change(container.querySelector("#configured-worker-name")!, { target: { value: "Aster" } });
  fireEvent.change(container.querySelector("#configured-worker-provider")!, { target: { value: "codex" } });
  fireEvent.change(container.querySelector("#configured-worker-repository")!, { target: { value: "/projects/aster" } });
  fireEvent.click(rendered.getByLabelText(/Use this path outside discovered project folders/));
  fireEvent.click(rendered.getByRole("button", { name: "Add sleeping worker" }));
  expect(onCreate).toHaveBeenCalledWith("Aster", "/projects/aster", "codex", true);
});

test("desktop drag ordering keeps accessible arrow controls as a fallback", () => {
  const onReorder = vi.fn().mockResolvedValue(undefined);
  const { container } = render(
    <WorkerSettings
      workers={[queen, budget, studio]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: false }}
      onCreate={vi.fn()}
      onUpdate={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn()}
      onReorder={onReorder}
    />,
  );
  const rendered = within(container);
  const source = rendered.getByRole("button", { name: "Move Poppy earlier" }).closest(".configured-worker")!;
  const target = rendered.getByRole("button", { name: "Move Daisy later" }).closest(".configured-worker")!;
  const dataTransfer = { effectAllowed: "", setData: vi.fn() };
  fireEvent.dragStart(source, { dataTransfer });
  fireEvent.dragOver(target, { dataTransfer });
  expect(target).toHaveClass("drop-target-before");
  fireEvent.drop(target, { dataTransfer });
  expect(onReorder).toHaveBeenCalledWith([studio.id, budget.id]);
  expect(rendered.getByRole("button", { name: "Move Poppy earlier" })).toBeInTheDocument();
});

test("drafts private repository context into an editable unsaved description", async () => {
  const onDraftDescription = vi.fn().mockResolvedValue("BudgetBug owns personal budget planning and bill tracking.");
  render(
    <WorkerSettings
      workers={[queen, budget]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={vi.fn()}
      onUpdate={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={onDraftDescription}
      onReorder={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(screen.getByRole("button", { name: "Draft from repository" }));
  expect(await screen.findByDisplayValue("BudgetBug owns personal budget planning and bill tracking.")).toBeInTheDocument();
  expect(onDraftDescription).toHaveBeenCalledWith(budget.id);
  expect(screen.getByText(/local README and project metadata only/)).toBeInTheDocument();
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
