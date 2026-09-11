import type { ConversationRecoveryOutcome } from "../api";

/**
 * What a settled conversation recovery is allowed to claim.
 *
 * ⚠️ TWO DIFFERENT CLAIMS WERE SAID WITH ONE SENTENCE. `restored` carries
 * `via_continue`, and the panel ignored it — so "Provider context was restored
 * at startup. That conversation was saved as the resumption default." appeared
 * both when Swarm asked for a specific conversation and got that exact one
 * back, and when the saved conversation was unavailable and the provider's own
 * continuation returned whichever conversation it felt was most recent.
 *
 * The domain is careful about this and the screen was not: `via_continue: false`
 * is produced only when the returned id EQUALS the chosen one, and anything else
 * from an exact attempt becomes `Manual`. A continuation is never matched
 * against a choice at all, so it is provenance, not verification.
 *
 * The ticket asks that safe recovery, continue and fresh fallback each be
 * distinguishable. Two of the three were.
 */
export function recoveryOutcomeWording(
  outcome: ConversationRecoveryOutcome,
  supersededByLaterSelection: boolean,
): string {
  if (supersededByLaterSelection) {
    return `Startup result: ${outcome.state}. The later confirmed selection above supersedes it.`;
  }
  switch (outcome.state) {
    case "restored":
      return outcome.via_continue
        ? "The saved conversation was unavailable, so the provider's own continuation was used. This is the conversation it returned; Swarm did not verify it against the one you chose."
        : "Provider context was restored at startup. That conversation was saved as the resumption default.";
    case "fresh":
      return "A fresh conversation started after recovery attempts. Previous context was not restored. Use the provider's resume command to choose another conversation.";
    default:
      return "Swarm could not confirm the intended conversation. The saved default was not changed. Check this terminal and use the provider's resume command if needed.";
  }
}

/**
 * The one-line note beside the terminal, for outcomes worth seeing without
 * opening Session details.
 *
 * A continuation earns one for the same reason `fresh` and `manual` do: what
 * came back was not checked against what was asked for. An exact restoration is
 * the only outcome that needs no note, because it is the only one where the
 * conversation on screen is the conversation that was chosen.
 */
export function recoveryOutcomeNote(
  outcome: ConversationRecoveryOutcome,
): string | null {
  switch (outcome.state) {
    case "restored":
      return outcome.via_continue
        ? "Continued conversation · not verified as the one you chose"
        : null;
    case "fresh":
      return "Fresh conversation · previous context not restored";
    default:
      return "Check conversation · see Session details";
  }
}
