import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { Task } from "../api";
import TaskDetailDialog from "./TaskDetailDialog";

afterEach(cleanup);

const task: Task = {
  id: "task-1", hive_id: "hive-1", title: "Repair the worker picker", workspace: "/workspace/swarm",
  state: "draft", description: "Keep the roster easy to scan", priority: "normal",
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
