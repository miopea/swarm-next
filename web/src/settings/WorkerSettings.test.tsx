import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { Worker } from "../api";
import WorkerSettings from "./WorkerSettings";

const queen = worker("queen", "Queen", "/projects/queen", 0, "queen");
const budget = worker("budget", "Daisy", "/projects/budgetbug", 1);
const studio = worker("studio", "Poppy", "/projects/sculpt-studio", 2);

afterEach(cleanup);

test.each(["cancel", "discard", "retry"])("returns focus to the edited worker after %s", async (action) => {
  const onUpdate = vi.fn().mockRejectedValueOnce(new Error("Save failed; retry is available")).mockResolvedValue(undefined);
  render(<WorkerSettings workers={[budget, studio]} workspaces={[]} busy={false}
    providers={{ claude_code: true, codex: true }} onCreate={vi.fn()} onUpdate={onUpdate}
    onChooseMark={vi.fn()} onRemove={vi.fn()} onDraftDescription={vi.fn()} onReorder={vi.fn()} />);
  const row = within(screen.getByRole("group", { name: "Worker Poppy" }));
  expect(row.getByRole("button", { name: "Edit" })).not.toHaveFocus();
  fireEvent.click(row.getByRole("button", { name: "Edit" }));
  const name = row.getByLabelText("Worker name");
  expect(name).toHaveFocus();
  if (action === "discard") fireEvent.change(name, { target: { value: "Poppy draft" } });
  if (action === "retry") {
    fireEvent.click(row.getByRole("button", { name: "Save worker" }));
    expect(await row.findByRole("alert")).toHaveTextContent("Save failed");
    expect(row.getByRole("form", { name: "Edit Poppy" })).toBeInTheDocument();
    fireEvent.click(row.getByRole("button", { name: "Save worker" }));
  } else {
    fireEvent.click(row.getByRole("button", { name: "Cancel" }));
    if (action === "discard") fireEvent.click(row.getByRole("button", { name: "Discard changes" }));
  }
  await waitFor(() => expect(row.getByRole("button", { name: "Edit" })).toHaveFocus());
  expect(onUpdate).toHaveBeenCalledTimes(action === "retry" ? 2 : 0);
});

test("worker controls remain grouped by identity through editing and cancellation", () => {
  const onUpdate = vi.fn();
  render(<WorkerSettings workers={[budget, studio]} workspaces={[]} busy={false}
    providers={{ claude_code: true, codex: true }} onCreate={vi.fn()} onUpdate={onUpdate}
    onChooseMark={vi.fn()} onRemove={vi.fn()} onDraftDescription={vi.fn()} onReorder={vi.fn()} />);
  const daisy = within(screen.getByRole("group", { name: "Worker Daisy" }));
  const poppy = within(screen.getByRole("group", { name: "Worker Poppy" }));
  expect(daisy.getByRole("button", { name: "Move Daisy later" })).toBeEnabled();
  fireEvent.click(poppy.getByRole("button", { name: "Edit" }));
  expect(poppy.getByRole("form", { name: "Edit Poppy" })).toBeInTheDocument();
  expect(daisy.queryByRole("form")).not.toBeInTheDocument();
  fireEvent.click(poppy.getByRole("button", { name: "Cancel" }));
  expect(poppy.getByRole("button", { name: "Edit" })).toBeInTheDocument();
  expect(onUpdate).not.toHaveBeenCalled();
});

test.each([false, true])("repository setup distinguishes empty discovery from assigned repositories (%s)", (discovered) => {
  render(<WorkerSettings workers={[]} workspaces={discovered
    ? [{ name: "trial", path: "/projects/trial", kind: "repository", configured_worker_id: "existing" }] : []}
    busy={false} providers={{ claude_code: true, codex: true }}
    onCreate={vi.fn()} onUpdate={vi.fn()} onChooseMark={vi.fn()} onRemove={vi.fn()}
    onDraftDescription={vi.fn()} onReorder={vi.fn()} />);
  expect(screen.getByText(discovered ? /Every discovered repository already has a worker/ : /No repositories were discovered/))
    .toHaveTextContent(/full path/);
  expect(screen.queryByText(/configuration will live/)).not.toBeInTheDocument();
  if (!discovered) expect(screen.queryByText(/Every discovered repository/)).not.toBeInTheDocument();
  fireEvent.change(screen.getByRole("combobox", { name: "Repository" }), { target: { value: "/projects/new" } });
  expect(screen.getByText(/No matching repository/)).toBeInTheDocument();
  expect(screen.getByLabelText(/Use this path outside discovered project folders/)).not.toBeChecked();
  expect(screen.getByRole("button", { name: "Add sleeping worker" })).toBeDisabled();
});

test("experimental creation requires opt-in and retains the selected provider when admission is withdrawn", async () => {
  const onCreate = vi.fn().mockRejectedValue(new Error("Provider availability changed"));
  const props = { workers: [], workspaces: [{ name: "trial", path: "/projects/trial", kind: "repository" as const, configured_worker_id: null }], busy: false,
    providers: { claude_code: true, codex: true, experimental: { gemini: true, grok: false, opencode: false } },
    onCreate, onUpdate: vi.fn(), onChooseMark: vi.fn(), onRemove: vi.fn(), onDraftDescription: vi.fn(), onReorder: vi.fn() };
  const { rerender } = render(<WorkerSettings {...props} />);
  const selector = screen.getByLabelText("Coding provider");
  expect(within(selector).queryByRole("option", { name: /Gemini/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByLabelText("Allow experimental providers for this change"));
  expect(within(selector).getByRole("option", { name: /Grok/ })).toBeDisabled();
  fireEvent.change(selector, { target: { value: "gemini" } });
  fireEvent.change(screen.getByLabelText("Worker name"), { target: { value: "Trial" } });
  fireEvent.change(screen.getByLabelText("Repository"), { target: { value: "/projects/trial" } });
  fireEvent.click(screen.getByRole("button", { name: "Add sleeping worker" }));
  expect(onCreate).toHaveBeenCalledWith("Trial", "/projects/trial", "gemini", false, true);
  expect(await screen.findByRole("alert")).toHaveTextContent("Provider availability changed");
  expect(screen.getByLabelText("Worker name")).toHaveValue("Trial");
  expect(selector).toHaveValue("gemini");

  fireEvent.click(screen.getByLabelText("Allow experimental providers for this change"));
  expect(selector).toHaveValue("gemini");
  expect(screen.getByRole("button", { name: "Add sleeping worker" })).toBeDisabled();
  fireEvent.click(screen.getByLabelText("Allow experimental providers for this change"));
  rerender(<WorkerSettings {...props} providers={{ claude_code: true, codex: true }} />);
  expect(selector).toHaveValue("gemini");
  expect(within(selector).getByRole("option", { name: /Gemini.*availability unknown/ })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Add sleeping worker" })).toBeDisabled();
});

test("failed worker edits keep the form and changed name available for correction", async () => {
  render(<WorkerSettings workers={[budget]} workspaces={[]} busy={false} providers={{ claude_code: true, codex: true }}
    onCreate={vi.fn()} onUpdate={vi.fn().mockRejectedValue(new Error("Worker could not be saved"))}
    onChooseMark={vi.fn()} onRemove={vi.fn()} onDraftDescription={vi.fn()} onReorder={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const editor = screen.getByRole("form", { name: "Edit Daisy" });
  fireEvent.change(within(editor).getByLabelText("Worker name"), { target: { value: "Daisy changed" } });
  fireEvent.click(within(editor).getByRole("button", { name: "Save worker" }));
  expect(await within(editor).findByRole("alert")).toHaveTextContent("Worker could not be saved");
  expect(within(editor).getByLabelText("Worker name")).toHaveValue("Daisy changed");
});

test("experimental provider change requires fresh consent after a completed edit", async () => {
  const onUpdate = vi.fn().mockResolvedValue(undefined);
  render(<WorkerSettings workers={[budget]} workspaces={[]} busy={false}
    providers={{ claude_code: true, codex: true, experimental: { gemini: true, grok: false, opencode: false } }}
    onCreate={vi.fn()} onUpdate={onUpdate} onChooseMark={vi.fn()} onRemove={vi.fn()} onDraftDescription={vi.fn()} onReorder={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  let editor = screen.getByRole("form", { name: "Edit Daisy" });
  fireEvent.click(within(editor).getByRole("checkbox", { name: "Allow experimental providers for this change" }));
  fireEvent.change(within(editor).getByLabelText("Default coding provider"), { target: { value: "gemini" } });
  fireEvent.click(within(editor).getByRole("button", { name: "Save worker" }));
  await waitFor(() => expect(screen.queryByRole("form", { name: "Edit Daisy" })).not.toBeInTheDocument());
  expect(onUpdate).toHaveBeenCalledWith(budget.id, budget.name, "", "gemini", budget.autostart, undefined, false, true);
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  editor = screen.getByRole("form", { name: "Edit Daisy" });
  expect(within(editor).getByRole("checkbox", { name: "Allow experimental providers for this change" })).not.toBeChecked();
  // Until refreshed worker data confirms the binding, this is still a change.
  expect(within(editor).getByLabelText("Default coding provider")).toHaveValue("gemini");
  expect(within(editor).getByRole("button", { name: "Save worker" })).toBeDisabled();
});

test("discarding an experimental edit clears its consent without changing the worker", () => {
  const onUpdate = vi.fn();
  render(<WorkerSettings workers={[budget]} workspaces={[]} busy={false}
    providers={{ claude_code: true, codex: true, experimental: { gemini: true, grok: false, opencode: false } }}
    onCreate={vi.fn()} onUpdate={onUpdate} onChooseMark={vi.fn()} onRemove={vi.fn()} onDraftDescription={vi.fn()} onReorder={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(within(screen.getByRole("form", { name: "Edit Daisy" })).getByRole("checkbox", { name: "Allow experimental providers for this change" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const editor = screen.getByRole("form", { name: "Edit Daisy" });
  expect(within(editor).getByRole("checkbox", { name: "Allow experimental providers for this change" })).not.toBeChecked();
  expect(within(editor).getByLabelText("Default coding provider")).toHaveValue("claude_code");
  expect(onUpdate).not.toHaveBeenCalled();
});

test.each(["gemini", "grok", "opencode"] as const)("preserves and identifies an existing %s worker instead of displaying Claude", (provider) => {
  const onUpdate = vi.fn().mockResolvedValue(undefined);
  render(<WorkerSettings workers={[{ ...budget, provider }]} workspaces={[]} busy={false}
    providers={{ claude_code: true, codex: true }} onCreate={vi.fn()} onUpdate={onUpdate}
    onChooseMark={vi.fn()} onRemove={vi.fn()} onDraftDescription={vi.fn()} onReorder={vi.fn()} />);
  fireEvent.change(screen.getByRole("searchbox", { name: "Find a worker" }), { target: { value: provider } });
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const editor = screen.getByRole("form", { name: "Edit Daisy" });
  const selector = within(editor).getByRole("combobox", { name: "Default coding provider" });
  expect(selector).toHaveValue(provider);
  expect(within(selector).getByRole("option", { name: /existing experimental provider/, selected: true })).toBeInTheDocument();
  expect(within(screen.getByLabelText("Coding provider")).queryByRole("option", { name: /experimental/ })).not.toBeInTheDocument();
  fireEvent.change(within(editor).getByLabelText("Worker name"), { target: { value: "Daisy renamed" } });
  fireEvent.click(within(editor).getByRole("button", { name: "Save worker" }));
  expect(onUpdate).toHaveBeenCalledWith(budget.id, "Daisy renamed", budget.description ?? "", provider, budget.autostart, undefined, false);
});

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
  expect(rendered.getByText(/No matching repository/)).toBeInTheDocument();
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
  const archivist = { ...worker("archivist", "Archivist", "/projects/archive", 0), system_role: "archivist" };
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
  const scout = { ...worker("scout", "Scout", "/projects", 0), system_role: "scout" };
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
