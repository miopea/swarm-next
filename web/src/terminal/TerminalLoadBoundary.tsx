import { Component, type ErrorInfo, type ReactNode } from "react";
import { recordClientFailure } from "../feedback/clientDiagnostics";

interface TerminalLoadBoundaryProps {
  children: ReactNode;
  onReload?: () => void;
}

interface TerminalLoadBoundaryState {
  failed: boolean;
}

export default class TerminalLoadBoundary extends Component<TerminalLoadBoundaryProps, TerminalLoadBoundaryState> {
  state: TerminalLoadBoundaryState = { failed: false };

  static getDerivedStateFromError(): TerminalLoadBoundaryState {
    return { failed: true };
  }

  componentDidCatch(_error: Error, _info: ErrorInfo) {
    recordClientFailure("react_render");
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <div className="terminal-empty" role="alert">
        <p className="eyebrow">Display interrupted</p>
        <h3>Swarm could not display this terminal</h3>
        <p>Check your connection and refresh to reconnect. Refreshing reloads this view; it does not restart the worker.</p>
        <button onClick={this.props.onReload ?? (() => window.location.reload())}>Refresh Swarm</button>
      </div>
    );
  }
}
