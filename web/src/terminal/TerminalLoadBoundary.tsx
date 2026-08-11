import { Component, type ErrorInfo, type ReactNode } from "react";

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

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("terminal view failed to load", error, info);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <div className="terminal-empty" role="alert">
        <p className="eyebrow">Terminal update available</p>
        <h3>Refresh to reconnect</h3>
        <p>Swarm was updated while this tab was open. Your worker is still running.</p>
        <button onClick={this.props.onReload ?? (() => window.location.reload())}>Refresh Swarm</button>
      </div>
    );
  }
}
