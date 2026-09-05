import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import { AppErrorBoundary } from "./app/AppErrorBoundary";
import "@xterm/xterm/css/xterm.css";
import "./app/styles.css";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Twominal root element was not found");
}

createRoot(root).render(
  <StrictMode>
    <AppErrorBoundary>
      <App />
    </AppErrorBoundary>
  </StrictMode>,
);
