import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { capitalizeFirst, renderInline, tokenizeInline } from "./inlineMarkdown";

describe("inline markdown in release notes", () => {
  it("renders bold rather than printing the asterisks", () => {
    render(<p>{renderInline("**Feedback goes to GitHub.** It files a real issue", "k")}</p>);
    expect(screen.getByText("Feedback goes to GitHub.").tagName).toBe("STRONG");
    // The failure being fixed: 1.0.0 shipped these as literal characters.
    expect(document.body.textContent).not.toContain("**");
  });

  it("renders backticked text as code", () => {
    render(<p>{renderInline("Check `cat VERSION` first", "k")}</p>);
    expect(screen.getByText("cat VERSION").tagName).toBe("CODE");
    expect(document.body.textContent).not.toContain("`");
  });

  it("leaves identifiers containing underscores and single asterisks alone", () => {
    // An italic rule would eat the middle of these. This is why there isn't one.
    const text = "Tables email_reply_deliveries and user_version are unchanged";
    render(<p>{renderInline(text, "k")}</p>);
    expect(document.body.textContent).toBe(text);
  });

  it("capitalises the first letter even when the bullet opens with bold", () => {
    const tokens = capitalizeFirst(tokenizeInline("**from 0.9.x** your workers keep running"));
    expect(tokens[0]).toEqual({ kind: "bold", value: "From 0.9.x" });
  });

  it("capitalises a plain opening word, as it always did", () => {
    const tokens = capitalizeFirst(tokenizeInline("importing email works again"));
    expect(tokens[0]).toEqual({ kind: "text", value: "Importing email works again" });
  });

  it("passes unmarked text straight through", () => {
    expect(tokenizeInline("nothing to see")).toEqual([{ kind: "text", value: "nothing to see" }]);
  });

  it("never produces markup from a note, whoever wrote it", () => {
    render(<p>{renderInline("**<img src=x onerror=alert(1)>** ok", "k")}</p>);
    expect(document.querySelector("img")).toBeNull();
    expect(document.body.textContent).toContain("<img src=x onerror=alert(1)>");
  });
});
