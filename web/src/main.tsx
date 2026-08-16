import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/atkinson-hyperlegible-mono";
import "@fontsource-variable/atkinson-hyperlegible-next";
import "@xterm/xterm/css/xterm.css";
import { App } from "./App";
import AppErrorBoundary from "./feedback/AppErrorBoundary";
import { installClientFailureCapture } from "./feedback/clientDiagnostics";
import ApiaryHandoffLanding from "./settings/ApiaryHandoffLanding";
import "./styles.css";

installClientFailureCapture();

createRoot(document.getElementById("root")!).render(
  <StrictMode><AppErrorBoundary><ApiaryHandoffLanding><App /></ApiaryHandoffLanding></AppErrorBoundary></StrictMode>,
);
