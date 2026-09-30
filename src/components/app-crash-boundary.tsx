import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { reportFrontendError } from "@/lib/crash-report";

export const APP_CRASH_TITLE = "keeper hit an error and stopped drawing this window.";
export const APP_CRASH_RELOAD = "Reload the window";

interface AppCrashBoundaryProps {
  children: ReactNode;
}

interface AppCrashBoundaryState {
  error: Error | null;
}

/**
 * The last line between a render error and an empty window.
 *
 * Without it React unmounts the whole root on an error nothing catches, and
 * what the person sees is the window's background and nothing else — no text,
 * no controls, an app that looks frozen while every process behind it is
 * idle. That is the owner's "unfold Properties and keeper hangs on an empty
 * window" report (epic 88), which a release build could not explain because
 * the only record of the error was a console nobody can open.
 *
 * So the error is said here, in the window, with a way back, and it is written
 * to the app log through {@link reportFrontendError}. Reloading the webview
 * costs the frontend's in-memory state only: everything that matters lives in
 * Rust and comes back with the first subscription.
 */
export class AppCrashBoundary extends Component<AppCrashBoundaryProps, AppCrashBoundaryState> {
  state: AppCrashBoundaryState = { error: null };

  static getDerivedStateFromError(error: unknown): AppCrashBoundaryState {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  componentDidCatch(error: unknown, info: ErrorInfo): void {
    reportFrontendError("render", error, info.componentStack ?? null);
  }

  render(): ReactNode {
    const { error } = this.state;
    if (error === null) {
      return this.props.children;
    }
    return (
      <main
        role="alert"
        className="flex h-dvh w-full flex-col items-center justify-center gap-3 bg-background p-6 text-foreground"
      >
        <p className="font-medium text-sm">{APP_CRASH_TITLE}</p>
        <p className="max-w-xl select-text break-words text-center font-mono text-meta text-muted-foreground">
          {error.message}
        </p>
        <Button type="button" onClick={() => window.location.reload()}>
          {APP_CRASH_RELOAD}
        </Button>
      </main>
    );
  }
}
