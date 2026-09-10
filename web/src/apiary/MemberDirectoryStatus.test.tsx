import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import MemberDirectoryStatus from "./MemberDirectoryStatus";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

test("an absent directory is incomplete, not an invitation to rejoin", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => new Response("null")));
  render(<MemberDirectoryStatus operatorToken="fictional" members={[]} onRefresh={vi.fn()} />);
  expect(await screen.findByText(/The full member list has not arrived/)).toHaveTextContent("do not leave and rejoin");
});

test("refresh failure retains dated snapshot and retry refreshes the roster", async () => {
  let fails = false;
  const refresh = vi.fn();
  vi.stubGlobal("fetch", vi.fn(async () => fails ? new Response("Unavailable", { status: 503 })
    : new Response(JSON.stringify({ revision: 1, issued_at: 100, expires_at: 400 }))));
  render(<MemberDirectoryStatus operatorToken="fictional" members={[]} onRefresh={refresh} />);
  expect(await screen.findByText(/Keeper directory issued/)).toHaveTextContent("saved snapshot, not live presence");
  expect(screen.getByText(/past its verification window/)).toBeInTheDocument();
  fails = true;
  fireEvent.click(screen.getByRole("button", { name: "Refresh member list" }));
  await screen.findByText(/Member list freshness could not be checked/);
  expect(screen.getByText(/Keeper directory issued/)).toBeInTheDocument();
  expect(refresh).toHaveBeenCalledOnce();
  fails = false;
  fireEvent.click(screen.getByRole("button", { name: "Refresh member list" }));
  await screen.findByRole("button", { name: "Refresh member list" });
  expect(screen.queryByText(/Member list freshness could not be checked/)).not.toBeInTheDocument();
});

test("malformed status is not described as a verified directory", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => new Response("[]")));
  render(<MemberDirectoryStatus operatorToken="fictional" members={[]} onRefresh={vi.fn()} />);
  await screen.findByText(/Member list freshness could not be checked/);
  expect(screen.queryByText(/Keeper directory issued/)).not.toBeInTheDocument();
});
