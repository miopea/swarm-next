import { useState } from "react";
import type { DecisionRequest } from "../api";
import DecisionInbox from "../decisions/DecisionInbox";
import { demoDecision, demoTasks, demoWorkers } from "./productFixtures";

/** In-memory only: exercise actual answer UI without resolving Hive decisions. */
export default function DecisionAnswerFixture() {
  const [decision, setDecision] = useState(demoDecision);
  const [failNext, setFailNext] = useState(true);
  const [receipt, setReceipt] = useState("");
  async function record(current: DecisionRequest, answer: string, note: string) {
    if (failNext) {
      setFailNext(false);
      throw new Error("Fixture connection failed. Your answer is still here to retry.");
    }
    setReceipt(JSON.stringify({ answer, note }));
    setDecision({ ...current, state: "resolved", resolution_action: "answered", resolution_note: answer });
  }
  return <div className="attention-workspace">
    <p>Isolated answer fixture. First send fails; retry records locally. No Hive requests.</p>
    <DecisionInbox decisions={[decision]} tasks={demoTasks} workers={demoWorkers} busy={false}
      onResolve={(current, action, note) => record(current, action, note)}
      onAnswer={(current, answers, note) => record(current, answers.Answer[0], note)} />
    {receipt && <output aria-label="Recorded fixture answer">{receipt}</output>}
  </div>;
}
