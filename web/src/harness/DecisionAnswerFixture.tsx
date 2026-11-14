import { useCallback, useRef, useState } from "react";
import type { DecisionImage, DecisionRequest } from "../api";
import DecisionInbox from "../decisions/DecisionInbox";
import type { SentImage } from "../decisions/answerImages";
import { demoDecision, demoTasks, demoWorkers } from "./productFixtures";

/** In-memory only: exercise actual answer UI without resolving Hive decisions. */
export default function DecisionAnswerFixture() {
  const [decision, setDecision] = useState(demoDecision);
  const [failNext, setFailNext] = useState(true);
  const [receipt, setReceipt] = useState("");
  // Screenshots "sent" here stay in this page, so the answered card can show
  // them the way it reads them back from a Hive.
  const kept = useRef(new Map<string, Blob>());
  async function record(current: DecisionRequest, answer: string, note: string, images: SentImage[] = []) {
    if (failNext) {
      setFailNext(false);
      throw new Error("Fixture connection failed. Your answer is still here to retry.");
    }
    setReceipt(JSON.stringify(images.length ? { answer, note, images: images.length } : { answer, note }));
    const recorded: DecisionImage[] = images.map((image, index) => {
      const id = `fixture-image-${index + 1}`;
      const bytes = Uint8Array.from(atob(image.data), (character) => character.charCodeAt(0));
      kept.current.set(id, new Blob([bytes], { type: image.media_type }));
      return { id, decision_id: current.id, clarification_id: null, media_type: image.media_type, byte_len: bytes.length, attached_at: 1 };
    });
    setDecision({ ...current, state: "resolved", resolution_action: "answered", resolution_note: answer, images: recorded });
  }
  const readImage = useCallback(async (_decision: string, image: string) => {
    const blob = kept.current.get(image);
    if (!blob) throw new Error("Not kept in this fixture");
    return blob;
  }, []);
  return <div className="attention-workspace">
    <p>Isolated answer fixture. First send fails; retry records locally. No Hive requests.</p>
    <DecisionInbox decisions={[decision]} tasks={demoTasks} workers={demoWorkers} busy={false}
      onResolve={(current, action, note, _surface, images) => record(current, action, note, images)}
      onAnswer={(current, answers, note, images) => record(current, answers.Answer[0], note, images)}
      onFetchImage={readImage} />
    {receipt && <output aria-label="Recorded fixture answer">{receipt}</output>}
  </div>;
}
