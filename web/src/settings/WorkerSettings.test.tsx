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
      onUpdate={onUpdate} onChooseMark={vi.fn()}
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
  expect(screen.getByText(/New workers receive a private Queen-routing draft/)).toBeInTheDocument();

  fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
  const editForm = screen.getByRole("form", { name: "Edit Daisy" });
  expect(within(editForm).getByLabelText("Repository")).toHaveValue(budget.workspace);
  fireEvent.change(within(editForm).getByLabelText("Worker name"), { target: { value: "Marigold" } });
  fireEvent.change(within(editForm).getByLabelText("Queen routing description"), { target: { value: "Owns budgets and bills." } });
  fireEvent.click(within(editForm).getByLabelText("Keep this worker active automatically"));
  fireEvent.click(within(editForm).getByRole("button", { name: "Save description to worker" }));
  expect(onUpdate).toHaveBeenCalledWith(budget.id, "Marigold", "Owns budgets and bills.", "claude_code", true, undefined, false);
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
      onUpdate={vi.fn()} onChooseMark={vi.fn()}
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
      onUpdate={vi.fn()} onChooseMark={vi.fn()}
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
      onUpdate={vi.fn()} onChooseMark={vi.fn()}
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
      onUpdate={vi.fn()} onChooseMark={vi.fn()}
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
  const onImproveDescription = vi.fn().mockResolvedValue("BudgetBug owns household budgeting, bills, and financial planning.");
  const onUpdate = vi.fn();
  render(
    <WorkerSettings
      workers={[queen, budget]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={vi.fn()}
      onUpdate={onUpdate} onChooseMark={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={onDraftDescription}
      onImproveDescription={onImproveDescription}
      onReorder={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(screen.getByRole("button", { name: "Draft locally" }));
  expect(await screen.findByDisplayValue("BudgetBug owns personal budget planning and bill tracking.")).toBeInTheDocument();
  expect(onDraftDescription).toHaveBeenCalledWith(budget.id);
  fireEvent.click(screen.getByRole("button", { name: "Generate with Claude" }));
  expect(await screen.findByDisplayValue("BudgetBug owns household budgeting, bills, and financial planning.")).toBeInTheDocument();
  expect(onImproveDescription).toHaveBeenCalledWith(budget.id);
  expect(screen.getByRole("status")).toHaveTextContent("Claude draft generated — save to apply it");
  expect(screen.getByRole("status")).toHaveTextContent("Queen cannot use this draft until it is saved");
  expect(screen.getByRole("button", { name: "Generate again with Claude" })).toBeInTheDocument();
  expect(onUpdate).not.toHaveBeenCalled();
  expect(screen.getByText(/one tool-free turn \(up to \$0.10\)/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Save description to worker" }));
  expect(onUpdate).toHaveBeenCalledWith(
    budget.id,
    budget.name,
    "BudgetBug owns household budgeting, bills, and financial planning.",
    budget.provider,
    budget.autostart,
    undefined,
    false,
  );
});

/// "There is no way to update the repo path, which I need to do for swarm
/// legacy."
///
/// A repository that moved on disk left its worker pointing at nothing, and the
/// only recorded path was rendered as text.
test("moves a sleeping worker to a repository that is not a discovered one", () => {
  const onUpdate = vi.fn().mockResolvedValue(undefined);
  render(
    <WorkerSettings
      workers={[queen, budget]}
      workspaces={[{ name: "budgetbug", path: budget.workspace, kind: "repository", configured_worker_id: budget.id }]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={vi.fn()}
      onUpdate={onUpdate} onChooseMark={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn()}
      onReorder={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const editForm = screen.getByRole("form", { name: "Edit Daisy" });
  const repository = within(editForm).getByLabelText("Repository");
  expect(repository).toHaveValue(budget.workspace);
  fireEvent.change(repository, { target: { value: "/projects/moved-budgetbug" } });

  // An unknown path needs the same consent a new worker's would, so the move
  // cannot be saved until it is given.
  expect(within(editForm).getByRole("button", { name: "Move worker" })).toBeDisabled();
  expect(within(editForm).getByText(/Moving a worker forgets its saved conversation/)).toBeInTheDocument();
  fireEvent.click(within(editForm).getByRole("checkbox", { name: /outside discovered project folders/ }));

  fireEvent.click(within(editForm).getByRole("button", { name: "Move worker" }));
  expect(onUpdate).toHaveBeenCalledWith(
    budget.id,
    budget.name,
    "",
    budget.provider,
    budget.autostart,
    "/projects/moved-budgetbug",
    true,
  );
});

test("will not move a running worker out from under itself", () => {
  render(
    <WorkerSettings
      workers={[queen, { ...budget, running: true, attention_state: "resting" }]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={vi.fn()}
      onUpdate={vi.fn()} onChooseMark={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn()}
      onReorder={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const runningForm = screen.getByRole("form", { name: "Edit Daisy" });
  expect(within(runningForm).getByLabelText("Repository")).toBeDisabled();
  expect(screen.getByText("Put this worker to sleep before moving it to another repository.")).toBeInTheDocument();
});

test("does not guess provider availability when the worker engine cannot be checked", () => {
  const onCreate = vi.fn();
  render(
    <WorkerSettings
      workers={[queen, budget]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: false }}
      providerCapabilitiesUnavailable
      onCreate={onCreate}
      onUpdate={vi.fn()} onChooseMark={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn()}
      onReorder={vi.fn()}
    />,
  );

  expect(screen.getByRole("alert")).toHaveTextContent("Coding providers could not be checked");
  expect(screen.getByLabelText("Coding provider")).toBeDisabled();
  expect(screen.getByRole("button", { name: "Add sleeping worker" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  expect(screen.getByRole("combobox", { name: "Default coding provider" })).toBeDisabled();
  expect(onCreate).not.toHaveBeenCalled();
});

test("protects a generated routing draft from an accidental cancel", async () => {
  render(
    <WorkerSettings
      workers={[queen, budget]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={vi.fn()}
      onUpdate={vi.fn()} onChooseMark={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn().mockResolvedValue("A useful routing draft.")}
      onReorder={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(screen.getByRole("button", { name: "Draft locally" }));
  expect(await screen.findByDisplayValue("A useful routing draft.")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.getByRole("alertdialog", { name: "Discard worker changes?" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Keep editing" })).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));
  expect(screen.getByDisplayValue("A useful routing draft.")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
  expect(screen.queryByRole("form", { name: "Edit Daisy" })).not.toBeInTheDocument();
});

test("filters a large roster without making hidden ordering ambiguous", () => {
  const onReorder = vi.fn().mockResolvedValue(undefined);
  render(
    <WorkerSettings
      workers={[queen, budget, studio]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={vi.fn()}
      onUpdate={vi.fn()} onChooseMark={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn()}
      onReorder={onReorder}
    />,
  );

  fireEvent.change(screen.getByLabelText("Find a worker"), { target: { value: "sculpt" } });
  expect(screen.getByText("Poppy")).toBeInTheDocument();
  expect(screen.queryByText("Daisy")).not.toBeInTheDocument();
  expect(screen.getByText("1 matching · clear the search to reorder")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Move Poppy earlier" })).not.toBeInTheDocument();
  expect(onReorder).not.toHaveBeenCalled();
});

test("shows the real Claude improvement failure instead of appearing inert", async () => {
  const onImproveDescription = vi.fn().mockRejectedValue(new Error("Runtime request returned 503: Claude Code is not available"));
  render(
    <WorkerSettings
      workers={[queen, budget]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={vi.fn()}
      onUpdate={vi.fn()} onChooseMark={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn()}
      onImproveDescription={onImproveDescription}
      onReorder={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(screen.getByRole("button", { name: "Generate with Claude" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Runtime request returned 503: Claude Code is not available");
});

/**
 * THE SET SENT TO THE SERVER MUST NOT CONTAIN A WORKER THE SERVER EXCLUDES.
 *
 * `reorder_workers` refuses anything that is not an exact set match against
 * `role != 'queen' AND system_role IS NULL` — ANY system role. The UI matched
 * on `system_role !== "scout"`, one named value, so the two agreed only while
 * scout was the only system role.
 *
 * A second one would render as an ordinary draggable row, be included in the
 * reorder payload, be excluded by the server, and every reorder would fail with
 * InvalidWorkerOrder. That is the 409 the operator hit on 2026-09-01 for a
 * different reason — connection-client workers — reproduced exactly by a
 * predicate disagreement rather than a hidden row.
 *
 * The test uses a system role that is deliberately NOT "scout", because a test
 * written with "scout" passes under both predicates and proves nothing.
 */
test("a system role other than scout is managed rather than reorderable", () => {
  const archivist = { ...worker("archivist", "Archivist", "/projects/archive", 0), system_role: "archivist" as unknown as "scout" };
  const onReorder = vi.fn();
  render(
    <WorkerSettings
      workers={[queen, archivist, budget]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={vi.fn()}
      onUpdate={vi.fn()} onChooseMark={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn()}
      onImproveDescription={vi.fn()}
      onReorder={onReorder}
    />,
  );

  // It is still shown — excluding it from the roster must not hide it.
  expect(screen.getByText("Archivist")).toBeTruthy();

  // ...and it is not draggable, which is what keeps it out of the payload.
  const row = screen.getByText("Archivist").closest("[draggable]");
  expect(row?.getAttribute("draggable")).not.toBe("true");
});

test("pins managed Scout after Queen while keeping provider and routing settings editable", () => {
  const scout = { ...worker("scout", "Scout", "/projects", 0), system_role: "scout" as const };
  render(
    <WorkerSettings
      workers={[queen, scout, budget]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={vi.fn()}
      onUpdate={vi.fn()} onChooseMark={vi.fn()}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn()}
      onReorder={vi.fn()}
    />,
  );

  const scoutSummary = screen.getByText("Scout").closest<HTMLElement>(".configured-worker")!;
  expect(scoutSummary).not.toHaveAttribute("draggable", "true");
  fireEvent.click(within(scoutSummary).getByRole("button", { name: "Edit" }));
  const form = screen.getByRole("form", { name: "Edit Scout" });
  expect(within(form).getByLabelText("Worker name")).toBeDisabled();
  expect(within(form).getByRole("combobox", { name: "Default coding provider" })).toBeEnabled();
  expect(within(form).queryByRole("button", { name: "Remove worker" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Move Scout later" })).not.toBeInTheDocument();
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

test("choosing a bee saves it on its own, without the rest of the form", async () => {
  // Everything else in this form is a draft the operator reviews before saving.
  // A bee is different: the only way to judge one is to see it worn, so it
  // applies on click. Sending it with the form would either save a half-typed
  // name or make choosing a bee wait for one.
  const onChooseMark = vi.fn();
  const onUpdate = vi.fn();
  render(
    <WorkerSettings
      workers={[worker("w1", "Platform", "/repo/platform", 0)]}
      workspaces={[]}
      busy={false}
      providers={{ claude_code: true, codex: true }}
      onCreate={vi.fn()}
      onUpdate={onUpdate}
      onChooseMark={onChooseMark}
      onRemove={vi.fn()}
      onDraftDescription={vi.fn()}
      onReorder={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /^Edit/ }));
  fireEvent.click(screen.getByRole("radio", { name: /Pigtails/ }));

  expect(onChooseMark).toHaveBeenCalledWith(expect.any(String), "pigtails");
  expect(onUpdate).not.toHaveBeenCalled();
});
