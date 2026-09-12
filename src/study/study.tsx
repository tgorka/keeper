import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import type { TelemetryStudyPreviewVm } from "@/lib/ipc/gen/TelemetryStudyPreviewVm";
import { telemetryStudyPreview, telemetryStudyStop } from "./commands";
import { type StudyCapture, startStudyCapture } from "./session";

const CAPTURE_FAILURE =
  "Study capture stopped because a request failed, replay was unavailable, or a safety limit was reached. Some data may already have been transmitted and cannot be recalled. Destination disclosure may remain until the app can confirm cleanup. Reload for another explicit attempt.";

export function Study() {
  const [destination, setDestination] = useState<TelemetryStudyPreviewVm | null>(null);
  const [phase, setPhase] = useState<"idle" | "starting" | "active" | "stopped" | "failed">("idle");
  const [leaving, setLeaving] = useState(false);
  const [lifecycleReady, setLifecycleReady] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [tab, setTab] = useState<"todo" | "done">("todo");
  const [done, setDone] = useState(false);
  const [compact, setCompact] = useState(false);
  const controller = useRef<AbortController | null>(null);
  const pending = useRef<Promise<StudyCapture> | null>(null);
  const capture = useRef<StudyCapture | null>(null);
  const navigationDialog = useRef<HTMLDialogElement | null>(null);
  const [navigation, setNavigation] = useState<"leave" | "reload">("leave");

  useEffect(() => {
    let alive = true;
    void telemetryStudyPreview()
      .then((status) => {
        if (alive) setDestination(status);
      })
      .catch(() => {});
    let unlisten: (() => void) | undefined;
    const registerClose = async () => {
      const dispose = await getCurrentWindow().onCloseRequested(async (event) => {
        event.preventDefault();
        controller.current?.abort();
        setPhase("stopped");
        // The native main window hides instead of unmounting. Destroy this study document
        // after stopping; a hidden window must not keep a recorder or retry queue alive.
        const finish = async () => {
          try {
            const running = capture.current ?? (await pending.current?.catch(() => null));
            if (running) await running.stop();
            else await telemetryStudyStop();
          } finally {
            window.location.replace("index.html");
          }
        };
        await finish().catch(() => {});
      });
      if (!alive) dispose();
      else {
        unlisten = dispose;
        setLifecycleReady(true);
      }
    };
    void registerClose().catch(() => {
      if (alive)
        setNotice(
          "Could not register the window-close safety guard. Study capture is unavailable.",
        );
    });
    const end = () => controller.current?.abort();
    // Capture phase runs before the SDK's bubble-phase unload flush handlers.
    window.addEventListener("beforeunload", end, { capture: true });
    window.addEventListener("pagehide", end, { capture: true });
    const hide = () => {
      if (document.visibilityState === "hidden" && controller.current) {
        end();
        setPhase("stopped");
      }
    };
    document.addEventListener("visibilitychange", hide, { capture: true });
    return () => {
      alive = false;
      end();
      unlisten?.();
      window.removeEventListener("beforeunload", end, { capture: true });
      window.removeEventListener("pagehide", end, { capture: true });
      document.removeEventListener("visibilitychange", hide, { capture: true });
    };
  }, []);

  useEffect(() => {
    if (phase !== "active") return;
    const timer = setTimeout(
      () => {
        controller.current?.abort();
        setPhase("stopped");
        setNotice("The ten-minute study limit ended capture.");
      },
      10 * 60 * 1_000,
    );
    return () => clearTimeout(timer);
  }, [phase]);

  const start = async () => {
    if (controller.current) return;
    const abort = new AbortController();
    controller.current = abort;
    setPhase("starting");
    setNotice(null);
    const promise = startStudyCapture(abort.signal, () => {
      abort.abort();
      setPhase("failed");
      setNotice(CAPTURE_FAILURE);
    });
    pending.current = promise;
    try {
      const running = await promise;
      if (abort.signal.aborted) {
        await running.stop();
        return;
      }
      capture.current = running;
      setPhase("active");
    } catch {
      if (!abort.signal.aborted) {
        setPhase("failed");
        setNotice(CAPTURE_FAILURE);
      }
    }
  };

  const stop = async () => {
    controller.current?.abort();
    setPhase("stopped");
    try {
      const running = capture.current ?? (await pending.current?.catch(() => null));
      if (running) await running.stop();
      else await telemetryStudyStop();
    } catch {
      setNotice(
        "Capture is stopped locally. The app could not confirm removal of the destination disclosure.",
      );
    }
  };

  const navigate = async () => {
    setLeaving(true);
    navigationDialog.current?.close();
    await stop();
    if (navigation === "reload") window.location.reload();
    else window.location.replace("index.html");
  };

  return (
    <main>
      <header id="study-controls" className="ph-no-capture">
        <h1>Synthetic usability study</h1>
        <p>
          This document contains only invented tasks. Your account, messages, drive, notes, bots and
          recordings are not loaded here. Never paste private content into this page.
        </p>
        <p>
          Start permits PostHog replay and click/movement heatmaps for this session only. Text and
          attributes are masked, inputs and media are blocked, and console values and network bodies
          are excluded. A fresh random study ID is not linked to your app installation. No AI
          analysis is enabled by this control.
        </p>
        <p>
          Capture stops on Stop, leaving, hiding the page, after ten minutes, or on a request
          failure or safety limit. Unsent data is discarded; data already transmitted cannot be
          recalled.
        </p>
        <p role="status">
          {destination?.configured
            ? `Study destination: ${destination.host}`
            : "Study is unavailable until this build supplies a configured destination."}
        </p>
        <p role="status">
          {phase === "active"
            ? "Replay and heatmaps active"
            : phase === "starting"
              ? "Starting replay and heatmaps…"
              : "Replay and heatmaps off"}
        </p>
        <nav aria-label="Study controls">
          <button
            type="button"
            disabled={!destination?.configured || !lifecycleReady || phase !== "idle" || leaving}
            onClick={() => {
              void start();
            }}
          >
            Start replay and heatmaps
          </button>
          <button
            type="button"
            disabled={!(phase === "starting" || phase === "active") || leaving}
            onClick={() => {
              void stop();
            }}
          >
            Stop capture
          </button>
          <button
            type="button"
            disabled={leaving}
            onClick={() => {
              setNavigation("leave");
              navigationDialog.current?.showModal();
            }}
          >
            Leave study
          </button>
          {(phase === "stopped" || phase === "failed") && (
            <button
              type="button"
              disabled={leaving}
              onClick={() => {
                setNavigation("reload");
                navigationDialog.current?.showModal();
              }}
            >
              Prepare a new session
            </button>
          )}
        </nav>
        {notice && <p role="alert">{notice}</p>}
        <dialog
          ref={navigationDialog}
          aria-labelledby="study-navigation-title"
          className="ph-no-capture"
        >
          <h2 id="study-navigation-title">
            {navigation === "leave" ? "Leave the study and reload the app?" : "Reload the study?"}
          </h2>
          <p>
            This replaces the whole document and discards this synthetic task board's changes.
            Capture stops before leaving. Returning to the app does not restore the previous view.
          </p>
          <button type="button" onClick={() => navigationDialog.current?.close()}>
            Stay here
          </button>
          <button
            type="button"
            onClick={() => {
              void navigate();
            }}
          >
            {navigation === "leave" ? "Leave study and reload app" : "Reload synthetic study"}
          </button>
        </dialog>
      </header>
      <section aria-label="Synthetic task board">
        <h2>Demo task board</h2>
        <p>Try moving the sample task to Done, opening Done, and switching the layout.</p>
        <nav aria-label="Demo task views">
          <button type="button" aria-pressed={tab === "todo"} onClick={() => setTab("todo")}>
            To do
          </button>
          <button type="button" aria-pressed={tab === "done"} onClick={() => setTab("done")}>
            Done
          </button>
          <button type="button" aria-pressed={compact} onClick={() => setCompact(!compact)}>
            Compact layout
          </button>
        </nav>
        {(tab === "done") === done ? (
          <article style={{ padding: compact ? "0.5rem" : "2rem" }}>
            <h3>Arrange the sample cards</h3>
            {!compact && <p>A made-up task for exploring this study's controls.</p>}
            <button type="button" onClick={() => setDone(!done)}>
              {done ? "Move to To do" : "Move to Done"}
            </button>
          </article>
        ) : (
          <p>No demo tasks in this view.</p>
        )}
      </section>
    </main>
  );
}
