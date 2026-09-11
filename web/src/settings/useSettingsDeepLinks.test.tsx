import { act, cleanup, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { useSettingsDeepLinks } from "./useSettingsDeepLinks";

afterEach(() => { cleanup(); window.history.replaceState({}, "", "/"); });

function visit(hash: string) {
  act(() => {
    window.history.replaceState({}, "", `/${hash}`);
    window.dispatchEvent(new HashChangeEvent("hashchange"));
  });
}

test("an Apiary Jira link opens Connections without remounting the app", () => {
  function Journey() {
    const [surface, setSurface] = useState("apiary");
    useSettingsDeepLinks(setSurface);
    return <p>{surface}</p>;
  }
  render(<Journey />);
  expect(screen.getByText("apiary")).toBeInTheDocument();
  visit("#settings-integrations");
  expect(screen.getByText("settings-connections")).toBeInTheDocument();
  visit("#settings-workers");
  expect(screen.getByText("settings-workers")).toBeInTheDocument();
});

test("unrelated anchors are ignored and the listener is removed on unmount", () => {
  const navigate = vi.fn();
  function Journey() { useSettingsDeepLinks(navigate); return null; }
  const view = render(<Journey />);
  visit("#worker-1");
  expect(navigate).not.toHaveBeenCalled();
  view.unmount();
  visit("#settings-integrations");
  expect(navigate).not.toHaveBeenCalled();
});
