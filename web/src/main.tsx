import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/atkinson-hyperlegible-mono";
import "@fontsource-variable/atkinson-hyperlegible-next";
import "@xterm/xterm/css/xterm.css";
import { App } from "./App";
import AppErrorBoundary from "./feedback/AppErrorBoundary";
import { installClientFailureCapture } from "./feedback/clientDiagnostics";
import { installBrowserPerformanceCapture } from "./runtime/browserPerformance";
import ApiaryHandoffLanding from "./settings/ApiaryHandoffLanding";
import "./styles.css";

installClientFailureCapture();
const stopBrowserPerformance = installBrowserPerformanceCapture();
import.meta.hot?.dispose(stopBrowserPerformance);

createRoot(document.getElementById("root")!).render(
  <StrictMode><AppErrorBoundary><ApiaryHandoffLanding><App /></ApiaryHandoffLanding></AppErrorBoundary></StrictMode>,
);
