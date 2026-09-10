import { useState } from "react";

import type { DecisionQuestion } from "../api";

const OTHER = Symbol("custom answer");

function optionDescription(question: DecisionQuestion, option: string): string {
  const descriptions = question.option_descriptions;
  return descriptions && Object.hasOwn(descriptions, option) ? descriptions[option] : "";
}

/**
 * Answering an interview-shaped decision request.
 *
 * The asker did not guess at the answers, so this does not present guesses as
 * the only choices: every question offers its options and a way to say
 * something else. An answer matching none of the options is the case the asker
 * failed to anticipate, which is the reason interviews exist.
 *
 * Every question must be answered before this submits. The asking worker is
 * holding its session for the whole set, and resuming it on a partial answer
 * would give it an incomplete picture with no way to ask the rest.
 */
type DecisionInterviewProps = {
  questions: DecisionQuestion[];
  busy: boolean;
  onAnswer: (answers: Record<string, string[]>, note: string) => void;
};

export default function DecisionInterview(props: DecisionInterviewProps) {
  // A refreshed object is not a new question. Changed wording, options, order,
  // or selection mode is: never apply an answer to a question not yet reviewed.
  // Key the complete form so even optional notes cannot survive a changed ask.
  const identity = JSON.stringify(props.questions.map((question) => [
    question.header, question.question, question.options, question.multi_select ?? false,
    question.options.map((option) => optionDescription(question, option)),
  ]));
  return <InterviewAnswers key={identity} {...props} />;
}

function InterviewAnswers({ questions, busy, onAnswer }: DecisionInterviewProps) {
  const [choices, setChoices] = useState<Map<string, (string | typeof OTHER)[]>>(() => new Map());
  const [other, setOther] = useState<Map<string, string>>(() => new Map());
  const [note, setNote] = useState("");

  const answerFor = (question: DecisionQuestion): string[] => {
    const chosen = choices.get(question.header) ?? [];
    const written = (other.get(question.header) ?? "").trim();
    const resolved = chosen.filter((value) => value !== OTHER);
    if (chosen.includes(OTHER) && written) resolved.push(written);
    return resolved;
  };
  const answers = Object.fromEntries(questions.map((q) => [q.header, answerFor(q)]));
  const unanswered = questions.filter((q) => answers[q.header].length === 0
    || ((choices.get(q.header) ?? []).includes(OTHER) && !(other.get(q.header) ?? "").trim()));

  function choose(question: DecisionQuestion, option: string | typeof OTHER) {
    setChoices((current) => {
      const held = current.get(question.header) ?? [];
      if (!question.multi_select) return new Map(current).set(question.header, [option]);
      return new Map(current).set(question.header, held.includes(option)
          ? held.filter((value) => value !== option)
          : [...held, option]);
    });
  }

  return (
    <div className="decision-interview">
      {questions.map((question) => {
        const held = choices.get(question.header) ?? [];
        return (
          <fieldset key={question.header} className="decision-question">
            <legend>{question.header}</legend>
            <p>{question.question}</p>
            <div className="decision-question-options" role="group" aria-label={question.header}>
              {question.options.map((option) => (
                <button
                  key={option}
                  type="button"
                  className={held.includes(option) ? "decision-option selected" : "decision-option"}
                  aria-pressed={held.includes(option)}
                  disabled={busy}
                  onClick={() => choose(question, option)}
                >
                  <span>{option}</span>
                  {optionDescription(question, option) ? (
                    <small className="decision-option-description">{optionDescription(question, option)}</small>
                  ) : null}
                </button>
              ))}
              <button
                type="button"
                className={held.includes(OTHER) ? "decision-option selected" : "decision-option"}
                aria-pressed={held.includes(OTHER)}
                disabled={busy}
                onClick={() => choose(question, OTHER)}
              >Something else</button>
            </div>
            {held.includes(OTHER) ? (
              <label>
                <span>Your answer</span>
                <input
                  required
                  value={other.get(question.header) ?? ""}
                  maxLength={200}
                  disabled={busy}
                  onChange={(event) => setOther((current) => new Map(current).set(question.header, event.target.value))}
                />
              </label>
            ) : null}
          </fieldset>
        );
      })}
      <details className="decision-argument decision-note">
        <summary>{note.trim() ? "Edit your note" : "Add an optional note"}</summary>
        <label>
          <span>Anything else the worker should know</span>
          <textarea value={note} maxLength={4000} disabled={busy} onChange={(event) => setNote(event.target.value)} placeholder="Optional" />
        </label>
      </details>
      <div className="decision-actions">
        <button
          type="button"
          className="primary-action"
          disabled={busy || unanswered.length > 0}
          onClick={() => onAnswer(answers, note)}
        >Send answers</button>
        {unanswered.length > 0 ? (
          <small role="status">
            {unanswered.length} of {questions.length} still to answer: {unanswered.map((q) => q.header).join(", ")}. Answer each question before sending.
          </small>
        ) : null}
      </div>
    </div>
  );
}
