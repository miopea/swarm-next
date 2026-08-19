import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { Task } from "../api";
import TaskDetailDialog from "./TaskDetailDialog";

afterEach(cleanup);

const task: Task = {
  id: "task-1", hive_id: "hive-1", title: "Repair the worker picker", workspace: "/workspace/swarm",
  state: "draft", description: "Keep the roster easy to scan", operator_instruction: "", priority: "normal",
  assigned_worker_id: null, assigned_session_id: null, position: 0, created_at: 1, updated_at: 1,
};

function renderDialog(onClose = vi.fn()) {
  render(<TaskDetailDialog task={task} operatorToken="token" busy={false} onClose={onClose} onSave={vi.fn()} onRemove={vi.fn()} />);
  return onClose;
}

test("guards edits from close, backdrop, and Escape until the operator chooses", () => {
  const onClose = renderDialog();
  fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Repair every worker picker" } });
  fireEvent.click(screen.getByRole("button", { name: "Close" }));

  expect(onClose).not.toHaveBeenCalled();
  expect(screen.getByRole("alertdialog", { name: "Unsaved task changes" })).toBeInTheDocument();
  const keepEditing = screen.getByRole("button", { name: "Keep editing" });
  expect(keepEditing).toHaveFocus();
  fireEvent.click(keepEditing);
  expect(screen.getByLabelText("Title")).toHaveValue("Repair every worker picker");

  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.getByRole("alertdialog", { name: "Unsaved task changes" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
  expect(onClose).toHaveBeenCalledOnce();
});

test("closes an unchanged task without an unnecessary warning", () => {
  const onClose = renderDialog();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(onClose).toHaveBeenCalledOnce();
  expect(screen.queryByRole("alertdialog", { name: "Unsaved task changes" })).not.toBeInTheDocument();
});

test("carries one operator instruction, apart from the brief", () => {
  // The operator regularly needs to say how a task should be approached —
  // "interview me first", "analyse this, do not act on it". Put in the brief it
  // reads as part of the work, and a worker can reasonably treat it that way.
  const save = vi.fn().mockResolvedValue(undefined);
  render(<TaskDetailDialog task={task} operatorToken="token" busy={false} onClose={vi.fn()} onSave={save} onRemove={vi.fn()} />);

  const instruction = screen.getByLabelText("How to approach this");
  expect(instruction).toHaveAttribute("maxLength", "280");
  fireEvent.change(instruction, { target: { value: "Interview me first" } });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

  expect(save).toHaveBeenCalledWith(expect.objectContaining({
    operator_instruction: "Interview me first",
  }));
});
