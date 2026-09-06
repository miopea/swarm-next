import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Worker } from "../api";
import ExperimentalHandoffDialog from "./ExperimentalHandoffDialog";

afterEach(cleanup);
const worker = { id: "parent", name: "Daisy", provider: "claude_code" } as Worker;
const providers = { claude_code: true, codex: true, experimental: { gemini: true, grok: false, opencode: false } };

test("temporary experimental handoff needs consent and preserves its choices on failure", async () => {
  const onConfirm = vi.fn().mockRejectedValueOnce(new Error("Engine unavailable")).mockResolvedValueOnce(undefined);
  const onClose = vi.fn();
  const props = { worker, provider: "gemini" as const, providers, capabilitiesUnavailable: false, onConfirm, onClose };
  const { rerender } = render(<ExperimentalHandoffDialog {...props} />);
  expect(screen.getByRole("button", { name: "Create temporary worker" })).toBeDisabled();
  fireEvent.click(screen.getByRole("checkbox"));
  expect(screen.getByText(/Swarm tools and automatic conversation recovery are not supported/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Create temporary worker" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Engine unavailable");
  expect(onClose).not.toHaveBeenCalled();
  expect(screen.getByRole("checkbox")).toBeChecked();
  rerender(<ExperimentalHandoffDialog {...props} providers={{ claude_code: true, codex: true }} />);
  expect(screen.getByRole("status")).toHaveTextContent("has not confirmed");
  expect(screen.getByRole("button", { name: "Create temporary worker" })).toBeDisabled();
  rerender(<ExperimentalHandoffDialog {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Create temporary worker" }));
  await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  expect(onConfirm).toHaveBeenCalledTimes(2);
});

test("unavailable discovery and failed capability refresh cannot admit a temporary worker", () => {
  const onConfirm = vi.fn();
  const props = { worker, provider: "grok" as const, providers, capabilitiesUnavailable: false, onConfirm, onClose: vi.fn() };
  const { rerender } = render(<ExperimentalHandoffDialog {...props} />);
  fireEvent.click(screen.getByRole("checkbox"));
  expect(screen.getByRole("status")).toHaveTextContent("not available");
  expect(screen.getByRole("button", { name: "Create temporary worker" })).toBeDisabled();
  rerender(<ExperimentalHandoffDialog {...props} provider="gemini" capabilitiesUnavailable />);
  expect(screen.getByRole("button", { name: "Create temporary worker" })).toBeDisabled();
  expect(onConfirm).not.toHaveBeenCalled();
});

test("pending creation cannot be duplicated or dismissed as though it were cancelled", async () => {
  let finish!: () => void;
  const onConfirm = vi.fn(() => new Promise<void>(resolve => { finish = resolve; }));
  const onClose = vi.fn();
  render(<ExperimentalHandoffDialog worker={worker} provider="gemini" providers={providers} capabilitiesUnavailable={false} onConfirm={onConfirm} onClose={onClose} />);
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "Create temporary worker" }));
  expect(screen.getByRole("button", { name: "Creating…" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(onClose).not.toHaveBeenCalled();
  expect(onConfirm).toHaveBeenCalledOnce();
  finish();
  await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
});
