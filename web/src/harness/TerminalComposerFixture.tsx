import { useState } from "react";
import { MobileTerminalComposer } from "../terminal/MobileTerminalComposer";
import type { TerminalConnectionState } from "../terminal/TerminalConnection";

/** Explicit fictional outcomes, not evidence of provider consumption or upload. */
export default function TerminalComposerFixture() {
  const [connection, setConnection] = useState<TerminalConnectionState>("connected");
  const [inputAvailable, setInputAvailable] = useState(true);
  const [refuseInput, setRefuseInput] = useState(false);
  const [failSource, setFailSource] = useState(false);
  const [writes, setWrites] = useState<string[]>([]);
  const [refreshes, setRefreshes] = useState(0);
  return <main style={{ padding: 12 }}>
    <h1>Fictional phone composer</h1>
    <p>No worker, upload or Hive. Controls below simulate transport outcomes.</p>
    <label>Connection <select value={connection} onChange={(event) => setConnection(event.target.value as TerminalConnectionState)}>
      <option value="connected">Connected</option>
      <option value="connecting">Connecting</option>
      <option value="disconnected">Disconnected</option>
    </select></label>
    <label><input type="checkbox" checked={!inputAvailable} onChange={(event) => setInputAvailable(!event.target.checked)} /> Viewing only</label>
    <label><input type="checkbox" checked={refuseInput} onChange={(event) => setRefuseInput(event.target.checked)} /> Refuse terminal input</label>
    <label><input type="checkbox" checked={failSource} onChange={(event) => setFailSource(event.target.checked)} /> Fail source record</label>
    <button type="button" onClick={() => document.documentElement.dataset.theme = document.documentElement.dataset.theme === "dark" ? "light" : "dark"}>Toggle fixture theme</button>
    <MobileTerminalComposer connectionState={connection} inputAvailable={inputAvailable}
      onInput={(text) => {
        if (refuseInput) return false;
        setWrites((previous) => [...previous, text].slice(-12));
        return true;
      }}
      onRecordSubmission={async () => { if (failSource) throw new Error("fictional source failure"); }}
      onRefresh={() => setRefreshes((previous) => previous + 1)} />
    <output aria-label="Fictional terminal writes">{JSON.stringify(writes)}</output>
    <p>Fictional redraws: {refreshes}</p>
  </main>;
}
