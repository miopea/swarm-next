import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import ProviderReleaseAction from "./ProviderReleaseAction";

afterEach(cleanup);

test("says how many workers are still running the release disk moved past", () => {
  // Claude reports "Update installed · Restart to update" in its own terminal.
  // Nothing said how many workers that applied to, so an update could be
  // installed and running nowhere.
  render(<ProviderReleaseAction busy={false} onRestart={vi.fn()} superseded={[
    { provider: "claude_code", version: "2.1.236", installed_at: 1_000, worker_ids: ["a", "b"], worker_names: { a: "Aster", b: "Bramble" }, in_place_worker_ids: [], asked_worker_ids: [] },
  ]} />);

  const card = screen.getByLabelText("Provider release status");
  expect(card).toHaveTextContent("Claude 2.1.236 is installed");
  expect(card).toHaveTextContent("2 running workers started before that");
  expect(card).toHaveTextContent("still running the older release");
});

test("counts a worker once when more than one provider is behind", () => {
  render(<ProviderReleaseAction busy={false} onRestart={vi.fn()} superseded={[
    { provider: "claude_code", version: "2.1.236", installed_at: 1_000, worker_ids: ["a", "b"], worker_names: { a: "Aster", b: "Bramble" }, in_place_worker_ids: [], asked_worker_ids: [] },
    { provider: "codex", version: null, installed_at: 1_000, worker_ids: ["c"], worker_names: { c: "Clover" }, in_place_worker_ids: [], asked_worker_ids: [] },
  ]} />);

  expect(screen.getByRole("button", { name: "Restart 3 workers" })).toBeInTheDocument();
});

test("confirms before interrupting, and says what is lost", async () => {
  const onRestart = vi.fn().mockResolvedValue(undefined);
  render(<ProviderReleaseAction busy={false} onRestart={onRestart} superseded={[
    { provider: "claude_code", version: "2.1.236", installed_at: 1_000, worker_ids: ["a"], worker_names: { a: "Aster" }, in_place_worker_ids: [], asked_worker_ids: [] },
  ]} />);

  fireEvent.click(screen.getByRole("button", { name: "Restart 1 worker" }));
  const confirm = screen.getByRole("group", { name: "Confirm provider restart" });
  expect(confirm).toHaveTextContent("interrupted and is not resumed");
  expect(confirm).toHaveTextContent("already on the installed release are left alone");

  fireEvent.click(screen.getByRole("button", { name: "Restart and update" }));
  expect(onRestart).toHaveBeenCalledOnce();
});

test("an update Swarm applies by itself offers no restart and interrupts nothing", () => {
  const onRestart = vi.fn();
  render(<ProviderReleaseAction busy={false} onRestart={onRestart} superseded={[
    { provider: "claude_code", version: "2.1.310", installed_at: 1_000, worker_ids: ["a", "b"], worker_names: { a: "Aster", b: "Bramble" }, in_place_worker_ids: ["a", "b"], asked_worker_ids: [] },
  ]} />);

  const card = screen.getByLabelText("Provider release status");
  expect(card).toHaveTextContent("Swarm restarts each one onto it with /restart");
  expect(card).toHaveTextContent("Nothing is interrupted");
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});

test("a worker Claude refused is asked again at once, without a confirmation", () => {
  const onRestart = vi.fn().mockResolvedValue(undefined);
  render(<ProviderReleaseAction busy={false} onRestart={onRestart} superseded={[
    { provider: "claude_code", version: "2.1.310", installed_at: 1_000, worker_ids: ["a"], worker_names: { a: "Aster" }, in_place_worker_ids: ["a"], asked_worker_ids: ["a"] },
  ]} />);

  expect(screen.getByRole("status")).toHaveTextContent("refuses while Remote Control is connected");
  fireEvent.click(screen.getByRole("button", { name: "Ask again" }));
  expect(onRestart).toHaveBeenCalledOnce();
});

/**
 * The operator read that one worker was not updated, and nothing said which. Each worker is
 * named where it is counted, and one Claude refused is named as refused.
 */
test("names each worker still on the older release, and each one Claude refused", () => {
  render(<ProviderReleaseAction busy={false} onRestart={vi.fn()} superseded={[
    { provider: "claude_code", version: "2.1.310", installed_at: 1_000, worker_ids: ["a", "b"], worker_names: { a: "Aster", b: "Bramble" }, in_place_worker_ids: ["a", "b"], asked_worker_ids: ["b"] },
    { provider: "codex", version: null, installed_at: 1_000, worker_ids: ["c"], worker_names: { c: "Clover" }, in_place_worker_ids: [], asked_worker_ids: [] },
  ]} />);

  const card = screen.getByLabelText("Provider release status");
  expect(card).toHaveTextContent("1 running worker started before that: Clover.");
  expect(card).toHaveTextContent("Swarm moves the other 1 worker onto it by itself, with /restart, without interrupting them: Aster.");
  expect(screen.getByRole("status")).toHaveTextContent("Claude did not restart Bramble when asked. Restarting asks it again.");
});

test("names the workers Swarm is moving by itself, and the one Claude refused", () => {
  render(<ProviderReleaseAction busy={false} onRestart={vi.fn()} superseded={[
    { provider: "claude_code", version: "2.1.310", installed_at: 1_000, worker_ids: ["a", "b"], worker_names: { a: "Aster", b: "Bramble" }, in_place_worker_ids: ["a", "b"], asked_worker_ids: ["b"] },
  ]} />);

  expect(screen.getByLabelText("Provider release status")).toHaveTextContent("2 workers still run the older release: Aster and Bramble.");
  expect(screen.getByRole("status")).toHaveTextContent("Claude did not restart Bramble when asked.");
});

test("says nothing at all when every worker is on the installed release", () => {
  const { container } = render(<ProviderReleaseAction busy={false} onRestart={vi.fn()} superseded={[]} />);

  expect(container).toBeEmptyDOMElement();
});
