import { useEffect, useState } from "react";
import { fetchJiraReadiness, type JiraProject, type JiraReadiness } from "../api";
import JiraSettings from "./JiraSettings";

/** One inline setup surface. Membership stays explicit after setup changes. */
export default function ApiaryJiraSetup({ operatorToken, projects, onChanged }: {
  operatorToken: string;
  projects: JiraProject[];
  onChanged: () => void;
}) {
  const [attempt, setAttempt] = useState(0);
  const [readiness, setReadiness] = useState<JiraReadiness>();
  const [unavailable, setUnavailable] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    let cancelled = false;
    const deadline = window.setTimeout(() => controller.abort(), 8_000);
    setUnavailable(false);
    void fetchJiraReadiness(operatorToken, controller.signal).then((next) => {
      if (!cancelled) setReadiness(next);
    }).catch(() => {
      if (!cancelled) setUnavailable(true);
    }).finally(() => window.clearTimeout(deadline));
    return () => { cancelled = true; controller.abort(); window.clearTimeout(deadline); };
  }, [operatorToken, attempt]);

  return <JiraSettings operatorToken={operatorToken} readiness={readiness}
    unavailable={unavailable} setupId="apiary-jira-setup" suggestedProjects={projects}
    onRetryReadiness={() => setAttempt((value) => value + 1)}
    onReadinessChanged={() => { setAttempt((value) => value + 1); onChanged(); }} />;
}
