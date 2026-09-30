import { ThemeProvider } from "next-themes";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { AppCrashBoundary } from "./components/app-crash-boundary";
import { installGlobalErrorReporting, reportFrontendError } from "./lib/crash-report";
import "./index.css";

installGlobalErrorReporting();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement, {
  // Only reached when the boundary itself fails; the boundary reports its own.
  onUncaughtError: (error, info) => {
    reportFrontendError("root", error, info.componentStack ?? null);
  },
}).render(
  <React.StrictMode>
    <ThemeProvider attribute="class" defaultTheme="system" enableSystem disableTransitionOnChange>
      <AppCrashBoundary>
        <App />
      </AppCrashBoundary>
    </ThemeProvider>
  </React.StrictMode>,
);
