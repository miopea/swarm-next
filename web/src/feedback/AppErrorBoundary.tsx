import { Component, type ErrorInfo, type ReactNode } from "react";

import BeeMascot from "../brand/BeeMascot";
import { recordClientFailure } from "./clientDiagnostics";

type Props = { children: ReactNode };
type State = { failed: boolean };

export default class AppErrorBoundary extends Component<Props, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(_error: Error, _info: ErrorInfo) {
    recordClientFailure("react_render");
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <main className="app-crash-recovery">
        <BeeMascot expression="blocked" />
        <p className="eyebrow">Display interrupted</p>
        <h1>Swarm hit a problem drawing this view</h1>
        <p>Your workers are still running. Reload the control room to reconnect; a content-free failure marker will be available in dogfood feedback.</p>
        <button type="button" onClick={() => window.location.reload()}>Reload control room</button>
      </main>
    );
  }
}
