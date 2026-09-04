import type { assessPerformance } from "./performanceAssessment";

export default function PerformanceEvidence({ evidence }: { evidence: ReturnType<typeof assessPerformance> }) {
  return <section aria-label="Performance evidence" className="performance-evidence">
    <h4>{evidence.headline}</h4>
    <dl className="diagnostic-list">
      <div><dt>Browser</dt><dd>{evidence.browser_detail}</dd></div>
      <div><dt>Server</dt><dd>{evidence.server_detail}</dd></div>
    </dl>
    <p>{evidence.limitations}</p>
  </section>;
}
