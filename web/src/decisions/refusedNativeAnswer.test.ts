import { describe, expect, it } from "vitest";
import { refusedNativeAnswerNotice } from "./refusedNativeAnswer";
import type { NativeAnswerRefusal } from "../api";

const REASONS: NativeAnswerRefusal[] = [
  "unverified",
  "ambiguous",
  "no_longer_applicable",
  "conflicting",
];

describe("refusedNativeAnswerNotice", () => {
  it("always says an answer was seen, and always says what to do", () => {
    // The two halves the operator needs: their answer was not ignored, and the
    // question is still theirs to settle.
    for (const reason of REASONS) {
      const notice = refusedNativeAnswerNotice(reason);
      expect(notice, reason).toContain("An answer was typed in this worker's terminal");
      expect(notice, reason).toMatch(/Answer here/);
    }
  });

  it("gives each reason its own remedy rather than one generic line", () => {
    const notices = REASONS.map(refusedNativeAnswerNotice);
    expect(new Set(notices).size).toBe(REASONS.length);
  });

  it("does not blame the operator for an answer Swarm could not confirm", () => {
    // Absence of confirmation is Swarm failing to check, not the operator
    // answering wrongly, and older captures carry no confirmation at all.
    const notice = refusedNativeAnswerNotice("unverified");
    expect(notice).toContain("Swarm cannot treat it as yours");
    expect(notice).toContain("answer in the terminal again");
  });

  it("says plainly that an ambiguous answer cannot be attributed", () => {
    expect(refusedNativeAnswerNotice("ambiguous"))
      .toContain("cannot tell which one you answered");
  });
});
