import { useEffect, useRef, useState } from "react";

import { claimGithubConnection, disconnectGithub, startGithubConnection } from "../api";

/**
 * Offers to file this report as the person writing it, rather than anonymously.
 *
 * WHY IT IS HERE AND NOT ONLY IN SETTINGS. The thing it buys is the answer
 * coming back — "closing issues with someone on a valid personal account can
 * get a response back that the issue has been fixed" — and the moment that
 * matters to a person is the moment they are about to report something, not
 * some earlier visit to a settings screen they had no reason to open.
 *
 * IT NEVER BLOCKS SUBMITTING, and that is the whole shape of it. The operator's
 * requirement was "frictionless for them to submit feedback"; connecting is an
 * upgrade offered beside the button, never a gate in front of it. Someone who
 * ignores this entirely still files, anonymously, with no extra step — which is
 * the path a first-time reporter is on and the one that must stay easy.
 */
export default function GithubConnectPanel({ operatorToken, connection, onChanged }: {
  operatorToken: string;
  /** Undefined until asked, so nothing is promised before it is known. */
  connection: { connected: boolean; lapsed: boolean; login: string | null } | undefined;
  onChanged: () => void;
}) {
  const [invitation, setInvitation] = useState<{ user_code: string; verification_uri: string }>();
  const [problem, setProblem] = useState<string>();
  const [busy, setBusy] = useState(false);
  const polling = useRef<number | undefined>(undefined);

  // A poll left running after the dialog closes keeps asking GitHub about an
  // authorisation nobody is completing.
  useEffect(() => () => {
    if (polling.current !== undefined) window.clearTimeout(polling.current);
  }, []);

  function pollOnce(afterSeconds: number) {
    polling.current = window.setTimeout(() => {
      void claimGithubConnection(operatorToken)
        .then((result) => {
          if (result.state === "connected") {
            setInvitation(undefined);
            onChanged();
            return;
          }
          if (result.state === "waiting") {
            // GitHub reports "still typing" as an error field on a 200. Waiting
            // is the ordinary case and must not end the attempt.
            pollOnce(result.interval ?? afterSeconds);
            return;
          }
          setInvitation(undefined);
          setProblem(
            result.state === "declined"
              ? "That request was declined on GitHub."
              : "The code expired before it was used.",
          );
        })
        .catch(() => {
          setInvitation(undefined);
          setProblem("GitHub could not be reached.");
        });
    }, Math.max(1, afterSeconds) * 1000);
  }

  async function connect() {
    setBusy(true);
    setProblem(undefined);
    try {
      const started = await startGithubConnection(operatorToken);
      setInvitation({ user_code: started.user_code, verification_uri: started.verification_uri });
      pollOnce(started.interval);
    } catch (error) {
      setProblem(error instanceof Error ? error.message : "GitHub could not be reached.");
    } finally {
      setBusy(false);
    }
  }

  if (connection?.connected) {
    return (
      <p className="feedback-github-connection">
        Filed as <strong>{connection.login}</strong>, so GitHub tells you when it is closed.{" "}
        <button
          type="button"
          className="text-button"
          onClick={() => void disconnectGithub(operatorToken).then(onChanged)}
        >Disconnect</button>
      </p>
    );
  }

  if (invitation) {
    return (
      <p className="feedback-github-connection" role="status">
        Enter <strong className="feedback-github-code">{invitation.user_code}</strong> at{" "}
        <a href={invitation.verification_uri} target="_blank" rel="noreferrer">{invitation.verification_uri}</a>.
        {" "}Waiting for you there — this page picks it up on its own.
      </p>
    );
  }

  return (
    <p className="feedback-github-connection">
      {/* A LAPSE IS NOT THE SAME AS NEVER HAVING CONNECTED, and saying "this
          will be filed anonymously" to someone who deliberately connected reads
          as though they never did. They are owed the reason and the name of the
          account that stopped working — otherwise the first they know of it is
          that answers quietly stopped arriving. */}
      {connection?.lapsed ? (
        <>
          Your GitHub connection{connection.login ? <> as <strong>{connection.login}</strong></> : null}{" "}
          has expired, so this will be filed anonymously and nobody can reply to you.{" "}
        </>
      ) : (
        // Says what connecting BUYS rather than what it is. "Connect GitHub"
        // describes a mechanism; hearing back is the reason to bother.
        <>This will be filed anonymously, so nobody can reply to you.{" "}</>
      )}
      <button type="button" className="text-button" disabled={busy} onClick={() => void connect()}>
        {busy ? "Starting…" : connection?.lapsed ? "Reconnect GitHub" : "Connect GitHub to hear back"}
      </button>
      {problem ? <span className="feedback-github-problem"> {problem}</span> : null}
    </p>
  );
}
