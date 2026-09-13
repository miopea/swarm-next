import type { NativeAnswerRefusal } from "../api";

/**
 * What to tell the operator about an answer Swarm could not use.
 *
 * ⚠️ EACH REASON ASKS FOR SOMETHING DIFFERENT, which is why this is not one
 * sentence with a reason appended. Answer again where it can be confirmed;
 * answer here because Swarm cannot tell two questions apart; answer here because
 * the question moved. A single "that did not work" would leave the operator to
 * guess which.
 *
 * The whole complaint being fixed is "you answer in the terminal and Needs You
 * stays lit". A refusal nobody is told about reproduces that exactly.
 */
export function refusedNativeAnswerNotice(reason: NativeAnswerRefusal): string {
  switch (reason) {
    case "unverified":
      return "An answer was typed in this worker's terminal, but the engine did not confirm it as a completed answer, so Swarm cannot treat it as yours. Answer here, or answer in the terminal again.";
    case "ambiguous":
      return "An answer was typed in this worker's terminal, and more than one open question has exactly these options — so Swarm cannot tell which one you answered. Answer here to settle it.";
    case "never_completed":
      // ⚠️ SAYS NOTHING ABOUT HOW THEY ANSWERED, because Swarm does not know.
      // A typed reply and a cancelled interview leave the same trace: asked,
      // never completed. What is true of both is that nothing was captured and
      // that choosing an offered option is what a terminal can report.
      return "This was asked in the worker's terminal but never came back as a completed answer, so Swarm captured nothing to use. Answering there settles it only when you CHOOSE one of the offered options — typed text is not reported to Swarm. Choose an option there, or answer here.";
    case "conflicting":
      return "An answer was typed in this worker's terminal, but it is already recorded against a different question. Answer here.";
    default:
      return "An answer was typed in this worker's terminal, but this question or the worker's session changed before Swarm could use it. Answer here.";
  }
}
