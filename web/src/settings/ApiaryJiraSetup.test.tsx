import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ApiaryJiraSetup from "./ApiaryJiraSetup";

vi.mock("./JiraSettings", () => ({ default: ({ unavailable, readiness, onRetryReadiness, onReadinessChanged }: {
  unavailable: boolean; readiness?: { connection: string };
  onRetryReadiness: () => void; onReadinessChanged: () => void;
}) => <div><span>{unavailable ? "Unavailable" : readiness?.connection ?? "Loading"}</span>
  <button onClick={onRetryReadiness}>Retry</button><button onClick={onReadinessChanged}>Saved setup</button></div> }));

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

test("failed status can retry and successful setup tells enrollment to refresh without joining", async () => {
  let fail = true;
  const onChanged = vi.fn();
  const request = vi.fn(async () => fail ? new Response("offline", { status: 503 })
    : new Response(JSON.stringify({ connection: "ready" })));
  vi.stubGlobal("fetch", request);
  render(<ApiaryJiraSetup operatorToken="fictional" projects={[]} onChanged={onChanged} />);
  await screen.findByText("Unavailable");
  fail = false;
  fireEvent.click(screen.getByText("Retry"));
  await screen.findByText("ready");
  expect(onChanged).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("Saved setup"));
  expect(onChanged).toHaveBeenCalledOnce();
  await waitFor(() => expect(request).toHaveBeenCalledTimes(3));
});

test("unmount aborts its outstanding status request", async () => {
  let signal: AbortSignal | undefined;
  vi.stubGlobal("fetch", vi.fn((_input: unknown, init?: RequestInit) => {
    signal = init?.signal ?? undefined;
    return new Promise<Response>(() => undefined);
  }));
  const view = render(<ApiaryJiraSetup operatorToken="fictional" projects={[]} onChanged={vi.fn()} />);
  await waitFor(() => expect(signal).toBeDefined());
  view.unmount();
  expect(signal?.aborted).toBe(true);
});
