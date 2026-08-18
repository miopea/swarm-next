import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import JiraDiscussion from "./JiraDiscussion";

afterEach(() => cleanup());

test("recovers a failed discussion load without presenting an empty discussion", async () => {
  const onFetch = vi.fn()
    .mockRejectedValueOnce(new Error("Runtime request returned 502"))
    .mockResolvedValueOnce([{ id: "comment-1", author_name: "Bea", body: "Ready", created_at: "2026-08-18T12:00:00Z", updated_at: "2026-08-18T12:00:00Z" }]);

  render(<JiraDiscussion taskId="task-1" issueKey="WEB-42" onFetch={onFetch} onAdd={vi.fn()} />);

  expect(await screen.findByRole("alert")).toHaveTextContent("Jira discussion could not be loaded");
  expect(screen.queryByText("No Jira comments yet.")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Retry discussion" }));
  expect(await screen.findByText("Ready")).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("does not invite a duplicate post when Jira saves before refresh fails", async () => {
  const onFetch = vi.fn()
    .mockResolvedValueOnce([])
    .mockRejectedValueOnce(new Error("refresh failed"))
    .mockResolvedValueOnce([{ id: "comment-1", author_name: "Bradford", body: "Shipped", created_at: "2026-08-18T12:00:00Z", updated_at: "2026-08-18T12:00:00Z" }]);
  const onAdd = vi.fn().mockResolvedValue({ state: "delivered" });

  render(<JiraDiscussion taskId="task-1" issueKey="WEB-42" onFetch={onFetch} onAdd={onAdd} />);
  await waitFor(() => expect(onFetch).toHaveBeenCalledTimes(1));
  fireEvent.change(screen.getByLabelText("Add an update"), { target: { value: "Shipped" } });
  fireEvent.click(screen.getByRole("button", { name: "Share to Jira" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("Your update was saved");
  expect(screen.getByText("Shared to Jira.")).toBeInTheDocument();
  expect(screen.getByLabelText("Add an update")).toHaveValue("");
  expect(onAdd).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Retry discussion" }));
  expect(await screen.findByText("Shipped")).toBeInTheDocument();
  expect(onAdd).toHaveBeenCalledTimes(1);
});
