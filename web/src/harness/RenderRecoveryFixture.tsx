import { useState } from "react";
import AppErrorBoundary from "../feedback/AppErrorBoundary";
import TerminalLoadBoundary from "../terminal/TerminalLoadBoundary";

function BrokenView(): never { throw new Error("Fictional display failure; no Hive is connected."); }

export default function RenderRecoveryFixture() {
  const [recovered, setRecovered] = useState(false);
  const terminal = new URLSearchParams(location.search).get("layer") === "terminal";
  return <main style={{ padding: 16 }}>
    <h1>Fictional display recovery</h1>
    <p>No Hive, provider, update or worker restart. The terminal refresh action restores only this fixture.</p>
    {terminal ? recovered ? <p role="status">Fictional terminal view restored. No worker was restarted.</p>
      : <TerminalLoadBoundary onReload={() => setRecovered(true)}><BrokenView /></TerminalLoadBoundary>
      : <AppErrorBoundary><BrokenView /></AppErrorBoundary>}
  </main>;
}
