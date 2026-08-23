import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import OperatorAccessSettings from "./OperatorAccessSettings";

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  rotateOperatorToken: vi.fn(),
  createBrowserSession: vi.fn(),
}));

const api = await import("../api");

beforeEach(() => {
  vi.mocked(api.rotateOperatorToken).mockReset().mockResolvedValue(undefined);
  vi.mocked(api.createBrowserSession).mockReset().mockResolvedValue(undefined);
});
afterEach(cleanup);

/**
 * The operator lost an hour to a stale token in a password manager, with no
 * remedy but editing swarm.env and restarting a service — neither of which is
 * available from the phone they were holding.
 */
test("changes the token and signs this device straight back in", async () => {
  render(<OperatorAccessSettings busy={false} operatorToken="old-token" />);

  fireEvent.change(screen.getByLabelText("New token"), { target: { value: "a-much-longer-new-token" } });
  fireEvent.click(screen.getByRole("button", { name: "Change token" }));

  // It says what it costs before it does it.
  expect(screen.getByRole("button", { name: "Sign out everywhere and change it" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Sign out everywhere and change it" }));

  await waitFor(() => expect(api.rotateOperatorToken).toHaveBeenCalledWith("old-token", "a-much-longer-new-token"));
  // Without this the operator is locked out by their own rotation, since the
  // session making the request died with the old token.
  await waitFor(() => expect(api.createBrowserSession).toHaveBeenCalledWith("a-much-longer-new-token"));
  expect(await screen.findByText(/Token changed/)).toBeInTheDocument();
});

test("will not rotate to something too short to be a credential", () => {
  render(<OperatorAccessSettings busy={false} operatorToken="old-token" />);
  fireEvent.change(screen.getByLabelText("New token"), { target: { value: "short" } });
  expect(screen.getByRole("button", { name: "Change token" })).toBeDisabled();
});

test("does not rotate until the warning has been accepted", () => {
  render(<OperatorAccessSettings busy={false} operatorToken="old-token" />);
  fireEvent.change(screen.getByLabelText("New token"), { target: { value: "a-much-longer-new-token" } });
  fireEvent.click(screen.getByRole("button", { name: "Change token" }));
  expect(api.rotateOperatorToken).not.toHaveBeenCalled();
});

test("generates one rather than making the operator invent it", () => {
  render(<OperatorAccessSettings busy={false} operatorToken="old-token" />);
  fireEvent.click(screen.getByRole("button", { name: "Generate one" }));
  const generated = (screen.getByLabelText("New token") as HTMLInputElement).value;
  expect(generated).toMatch(/^[0-9a-f]{64}$/);
});
