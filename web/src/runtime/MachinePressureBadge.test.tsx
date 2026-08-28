import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import MachinePressureBadge from "./MachinePressureBadge";
import type { MachinePressureNotice } from "./machinePressure";

const notice = (level: MachinePressureNotice["level"], label: string): MachinePressureNotice =>
  ({ level, label, detail: "memory 96% used." });

describe("MachinePressureBadge", () => {
  it("renders nothing when there is nothing to say", () => {
    const { container } = render(<MachinePressureBadge notice={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  // THE COLOUR-INDEPENDENCE GUARD. The operator asked for "the heartbeat thing
  // changing colour or something"; colour alone fails anyone who cannot rely on
  // it and this app targets WCAG 2.1 AA. These assertions run with no
  // stylesheet at all, so anything they can still read is by definition not
  // carried by hue.
  it.each([
    ["advisory", "Machine under load"],
    ["critical", "Machine critical"],
    ["unknown", "Machine unknown"],
  ] as const)("states %s in words, not only in colour", (level, label) => {
    render(<MachinePressureBadge notice={notice(level, label)} />);
    expect(screen.getByRole("status")).toHaveTextContent(label);
  });

  it("gives each level its own shape, so the three differ in greyscale", () => {
    const glyphFor = (level: MachinePressureNotice["level"]) => {
      const { container, unmount } = render(<MachinePressureBadge notice={notice(level, "x")} />);
      const glyph = container.querySelector(".machine-pressure-glyph")?.innerHTML ?? "";
      unmount();
      return glyph;
    };
    const advisory = glyphFor("advisory");
    const critical = glyphFor("critical");
    const unknown = glyphFor("unknown");
    expect(advisory).not.toBe("");
    // Ablation: give any two levels the same artwork and this fails.
    expect(new Set([advisory, critical, unknown]).size).toBe(3);
  });

  it("carries the numbers to assistive technology as well as the tooltip", () => {
    render(<MachinePressureBadge notice={notice("critical", "Machine critical")} />);
    const badge = screen.getByRole("status");
    expect(badge).toHaveTextContent("memory 96% used.");
    expect(badge).toHaveAttribute("title", "Machine critical. memory 96% used.");
  });
});
