import { useCallback, useEffect, useState } from "react";

import { fetchConnections, revokeConnection, type Connection } from "../api";

/**
 * The outside tools connected to this Hive.
 *
 * WHY THIS EXISTS. A connected tool authenticates itself through OAuth and
 * registers itself, so nothing in the flow ever asks the operator to copy a
 * secret — which is what they asked for ("that way it stays in the clients").
 * The consequence is that without this page there is no list: a connection is
 * deliberately absent from the roster, so the only way to see one, or to take
 * it away, would be rotating the operator token and disconnecting everything at
 * once.
 *
 * REVOKING IS FINAL, and the copy says so, because it is not the reversible
 * kind of off. The profile is archived rather than deleted — the board writes it
 * already made still point at it — and its registration stays reserved, so the
 * same tool has to register again and be approved again.
 */
function whenText(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "unknown";
  return new Date(seconds * 1000).toLocaleString();
}

type Props = {
  operatorToken: string;
  /**
   * How the list is read. Defaults to the API.
   *
   * A seam, not a convenience: this card reads for itself, so without one the
   * harness cannot render it at all — and the state most worth LOOKING at is
   * the one where the read failed, which cannot be reached by any real API
   * response. Monkey-patching global fetch was tried first and silently showed
   * the failed state for every fixture.
   */
  load?: (operatorToken: string) => Promise<Connection[]>;
};

export default function ConnectionsSettings({ operatorToken, load: read = fetchConnections }: Props) {
  const [connections, setConnections] = useState<Connection[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);

  const load = useCallback(() => {
    // A failed read must not render as "no tools are connected" — that reads as
    // a safe answer and is not one.
    void read(operatorToken)
      .then((found) => { setConnections(found); setFailed(false); })
      .catch(() => setFailed(true));
  }, [operatorToken, read]);

  useEffect(load, [load]);

  async function revoke(id: string) {
    setBusy(id);
    try {
      await revokeConnection(operatorToken, id);
      setConfirming(null);
      load();
    } catch {
      setFailed(true);
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="settings-card" aria-labelledby="connections-heading">
      <p className="eyebrow">Connections</p>
      <h3 id="connections-heading">Outside tools</h3>
      <p className="settings-lead">
        Tools you have connected to this Hive over MCP — a desktop Claude client, an editor,
        anything that speaks it. Each one signs in through this Hive and is approved by you, and
        anything it does on the board is recorded under its own name.
      </p>

      {failed ? (
        <p className="connections-unknown" role="status">
          The connected tools could not be read, so this list may be incomplete.
          <button type="button" className="text-button" onClick={load}>Try again</button>
        </p>
      ) : null}

      {connections === null && !failed ? <p className="settings-lead">Checking…</p> : null}

      {connections?.length === 0 ? (
        <p className="settings-lead">
          Nothing is connected. To connect one, add <code>/mcp</code> on this Hive&apos;s address as a
          custom connector in the tool, and approve it when this Hive asks.
        </p>
      ) : null}

      {connections?.length ? (
        <ul className="connection-list">
          {connections.map((connection) => (
            <li key={connection.id} className="connection-row">
              <div className="connection-identity">
                <strong>{connection.name}</strong>
                <small>
                  Connected {whenText(connection.connected_at)} · last used {whenText(connection.last_seen_at)}
                </small>
              </div>
              {confirming === connection.id ? (
                <div className="connection-confirm">
                  <p>
                    Disconnect {connection.name}? It stops working immediately, and it cannot
                    reconnect without registering and being approved again.
                  </p>
                  <div className="connection-actions">
                    <button
                      type="button"
                      className="danger-button"
                      disabled={busy === connection.id}
                      onClick={() => void revoke(connection.id)}
                    >
                      {busy === connection.id ? "Disconnecting…" : "Disconnect it"}
                    </button>
                    <button type="button" onClick={() => setConfirming(null)}>Keep it</button>
                  </div>
                </div>
              ) : (
                <button
                  type="button"
                  className="secondary-button danger-text"
                  onClick={() => setConfirming(connection.id)}
                >
                  Disconnect
                </button>
              )}
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}
