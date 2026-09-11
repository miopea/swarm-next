import { describe, expect, it } from "vitest";
import { recoveryOutcomeNote, recoveryOutcomeWording } from "./conversationRecoveryWording";

describe("recoveryOutcomeWording", () => {
  it("claims a verified restoration only when the exact conversation came back", () => {
    const exact = recoveryOutcomeWording(
      { state: "restored", conversation: "chosen", via_continue: false },
      false,
    );
    expect(exact).toContain("Provider context was restored at startup");
    expect(exact).toContain("saved as the resumption default");
  });

  it("says a continuation was not checked against the conversation you chose", () => {
    // ⚠️ THE CONFLATION THIS EXISTS TO END. via_continue means the provider
    // picked; nothing compared its answer to the selection.
    const continued = recoveryOutcomeWording(
      { state: "restored", conversation: "whatever-was-latest", via_continue: true },
      false,
    );
    expect(continued).toContain("provider's own continuation");
    expect(continued).toContain("did not verify");
    expect(continued).not.toContain("Provider context was restored at startup");
  });

  it("never claims restored context for a fresh start or a manual stop", () => {
    expect(recoveryOutcomeWording({ state: "fresh", conversation: "new" }, false))
      .toContain("Previous context was not restored");
    expect(recoveryOutcomeWording({ state: "manual", reason: "uncertain_outcome" }, false))
      .toContain("could not confirm the intended conversation");
  });

  it("defers to a later confirmed selection whatever the startup did", () => {
    for (const outcome of [
      { state: "restored", conversation: "a", via_continue: false },
      { state: "restored", conversation: "a", via_continue: true },
      { state: "fresh", conversation: "b" },
      { state: "manual", reason: "provider_cannot_resume" },
    ] as const) {
      expect(recoveryOutcomeWording(outcome, true)).toContain("supersedes it");
    }
  });
});

describe("recoveryOutcomeNote", () => {
  it("gives every unverified outcome a note, and a verified one none", () => {
    expect(recoveryOutcomeNote({ state: "restored", conversation: "a", via_continue: false }))
      .toBeNull();
    expect(recoveryOutcomeNote({ state: "restored", conversation: "a", via_continue: true }))
      .toContain("not verified as the one you chose");
    expect(recoveryOutcomeNote({ state: "fresh", conversation: "b" }))
      .toContain("previous context not restored");
    expect(recoveryOutcomeNote({ state: "manual", reason: "fresh_start_failed" }))
      .toContain("Check conversation");
  });
});
