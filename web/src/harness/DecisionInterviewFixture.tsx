import { useEffect, useState } from "react";
import { applyColorTheme, type ColorTheme } from "../brand/theme";
import DecisionInterview from "../decisions/DecisionInterview";

/** Fictional, local-only interview for theme, keyboard and narrow-layout checks. */
export default function DecisionInterviewFixture() {
  const [theme, setTheme] = useState<ColorTheme>("dark");
  const [receipt, setReceipt] = useState("");
  useEffect(() => applyColorTheme(theme), [theme]);
  return <main className="attention-workspace">
    <h1>Fictional decision interview</h1>
    <p>No Hive request or worker message is sent.</p>
    <button className="secondary-button" onClick={() => setTheme(theme === "dark" ? "light" : "dark")}>Switch to {theme === "dark" ? "light" : "dark"} theme</button>
    <DecisionInterview busy={false} questions={[
      { header: "Recovery", question: "How should the fictional orchard export handle an incomplete source?", options: ["Fail the run", "Keep partial results"], option_descriptions: { "Fail the run": "Keep the previous export intact. Report which source needs attention; do not publish an incomplete replacement.", "Keep partial results": "Save a clearly marked local draft for inspection. Do not publish it or overwrite the previous export." } },
      { header: "Evidence", question: "Which evidence should the fictional check include?", options: ["Summary", "Retry count"], multi_select: true },
    ]} onAnswer={(answers, note) => setReceipt(JSON.stringify({ answers, note }))} />
    {receipt && <output aria-label="Recorded fictional answers">{receipt}</output>}
  </main>;
}
