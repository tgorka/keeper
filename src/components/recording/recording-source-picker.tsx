/**
 * The live Source card — application/window/display picker (Story 19.1, FR,
 * UX-DR29/DR30).
 *
 * A grouped, single-select {@link RadioGroup} under "Displays" then
 * "Applications" section headers. Each ~44px row shows a leading glyph (a lucide
 * `Monitor` for displays; the app's real `<img>` data-URI icon, or a lucide
 * `AppWindow` fallback when none), the source name, and radio semantics. The
 * selection is the ephemeral capture target the header Start passes to
 * `recording_start`, defaulting to the main display.
 *
 * The list is *live*: the picker polls `list_sources` on a fixed interval (~3s)
 * while mounted and again on window focus, showing a subtle "refreshing…"
 * affordance during an in-flight enumeration, and stops polling on unmount.
 *
 * When an application is selected, an inline disclosure states the exclusion
 * plainly (recording voice, sentence case): only that app's windows land in the
 * file — keeper, other apps, and notification banners stay out. A selection that
 * has vanished from the polled list is marked unavailable (Start against it fails
 * cleanly at the sidecar; the selection is never silently swapped).
 */
import { AppWindow, Monitor } from "lucide-react";
import { type ReactNode, useEffect } from "react";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import type { RecordingTargetVm } from "@/lib/ipc/client";
import { useSystemAudioEnabled } from "@/lib/stores/recording-audio";
import {
  isSameTarget,
  isSelectionAvailable,
  refreshRecordingSources,
  selectRecordingTarget,
  startRecordingSourcePolling,
  stopRecordingSourcePolling,
  useRecordingSources,
  useRecordingSourcesRefreshing,
  useSelectedRecordingTarget,
} from "@/lib/stores/recording-source";
import { cn } from "@/lib/utils";

/** The section headers (recording voice). */
export const DISPLAYS_HEADING = "Displays";
export const APPLICATIONS_HEADING = "Applications";

/** The main-display row label (the default target). */
export const MAIN_DISPLAY_LABEL = "Main display";

/** The subtle in-flight enumeration affordance. */
export const REFRESHING_LABEL = "Refreshing…";

/** The empty-applications hint (honest — no error, just nothing to list yet). */
export const NO_APPLICATIONS_NOTE = "No recordable applications yet.";

/** The unavailable-selection marker (the selected source vanished from the list). */
export const SELECTION_UNAVAILABLE_NOTE = "This source is no longer available.";

/** Test id for the picker root. */
export const SOURCE_PICKER_TESTID = "recording-source-picker";

/** The inline app-scope disclosure (Story 19.1) — `{App}` is interpolated.
 * Story 19.3 (deferred from 19.2): the "and audio" clause is honest only while
 * the Audio card's system-audio toggle is on — with system audio off, no app
 * audio lands in the file, so the clause is dropped. */
export function appScopeDisclosure(appName: string, systemAudioOn: boolean): string {
  const scope = systemAudioOn ? "windows and audio are" : "windows are";
  return `Only ${appName}'s ${scope} recorded — keeper, other apps, and notification banners stay out of the file.`;
}

/** Encode a target as a stable RadioGroup value string. */
function targetValue(target: RecordingTargetVm): string {
  return target.kind === "display"
    ? `display:${target.displayId ?? "main"}`
    : `application:${target.pid}`;
}

/** One ~44px source row: leading glyph + name + radio semantics. */
function SourceRow({
  target,
  name,
  glyph,
  selected,
  onSelect,
}: {
  target: RecordingTargetVm;
  name: string;
  glyph: ReactNode;
  selected: boolean;
  onSelect: () => void;
}) {
  const value = targetValue(target);
  return (
    // biome-ignore lint/a11y/noLabelWithoutControl: the RadioGroupItem is the control.
    <label
      className={cn(
        "flex min-h-11 cursor-pointer items-center gap-3 rounded-md px-2 text-sm",
        "hover:bg-muted/50",
        selected && "bg-muted/50",
      )}
    >
      <RadioGroupItem value={value} onClick={onSelect} />
      <span className="flex size-5 shrink-0 items-center justify-center text-muted-foreground">
        {glyph}
      </span>
      <span className="min-w-0 flex-1 truncate">{name}</span>
    </label>
  );
}

export function RecordingSourcePicker({ active = true }: { active?: boolean }) {
  const sources = useRecordingSources();
  const selected = useSelectedRecordingTarget();
  const refreshing = useRecordingSourcesRefreshing();
  // Story 19.3 (deferred from 19.2): the app-scope disclosure must not claim
  // the app's audio is recorded while the Audio card has system audio off.
  const systemAudioOn = useSystemAudioEnabled();

  // Poll while the idle setup surface is visible (`active`); stop while a session
  // is recording (`active === false`) or on unmount — otherwise a fresh
  // `keeper-rec` child would spawn every ~3s throughout an active recording. Also
  // re-enumerate on window focus (return-to-app), like the permission pre-flight.
  useEffect(() => {
    if (!active) {
      stopRecordingSourcePolling();
      return;
    }
    startRecordingSourcePolling();
    const onFocus = () => {
      void refreshRecordingSources();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
      stopRecordingSourcePolling();
    };
  }, [active]);

  const displays = sources?.displays ?? [];
  const applications = sources?.applications ?? [];
  const selectionAvailable = isSelectionAvailable(selected, sources);
  const selectedApp =
    selected.kind === "application"
      ? applications.find((app) => app.pid === selected.pid && app.bundleId === selected.bundleId)
      : undefined;

  const select = (target: RecordingTargetVm) => {
    // Radio semantics via the store (exactly one target selected app-wide).
    selectRecordingTarget(target);
  };

  // Decode a RadioGroup value string back into the full target (the app row
  // carries a `bundleId` the value string omits, so look it up). This is the
  // channel keyboard selection (arrow keys) flows through — Radix fires
  // `onValueChange`, not a row `onClick`, so without this the store would never
  // update for keyboard users and Start would record the stale target.
  const decodeTarget = (value: string): RecordingTargetVm | null => {
    if (value.startsWith("display:")) {
      const raw = value.slice("display:".length);
      return { kind: "display", displayId: raw === "main" ? null : Number(raw) };
    }
    if (value.startsWith("application:")) {
      const pid = Number(value.slice("application:".length));
      const app = applications.find((candidate) => candidate.pid === pid);
      return app ? { kind: "application", pid, bundleId: app.bundleId } : null;
    }
    return null;
  };

  return (
    <div className="flex flex-col gap-4" data-testid={SOURCE_PICKER_TESTID}>
      <div className="flex items-center justify-between">
        <span className="sr-only">Recording source</span>
        {refreshing && (
          <span className="text-muted-foreground text-xs" role="status">
            {REFRESHING_LABEL}
          </span>
        )}
      </div>

      <RadioGroup
        value={targetValue(selected)}
        onValueChange={(value) => {
          const target = decodeTarget(value);
          if (target !== null) {
            select(target);
          }
        }}
        aria-label="Recording source"
      >
        {/* Displays first. The main display is always individually selectable
            (the default); each enumerated display is its own row. */}
        <p className="font-medium text-muted-foreground text-xs">{DISPLAYS_HEADING}</p>
        <SourceRow
          target={{ kind: "display", displayId: null }}
          name={MAIN_DISPLAY_LABEL}
          glyph={<Monitor className="size-4" aria-hidden="true" />}
          selected={selected.kind === "display" && selected.displayId === null}
          onSelect={() => select({ kind: "display", displayId: null })}
        />
        {displays
          .filter((display) => !display.isMain)
          .map((display) => {
            const target: RecordingTargetVm = { kind: "display", displayId: display.id };
            return (
              <SourceRow
                key={`display-${display.id}`}
                target={target}
                name={`Display ${display.id} (${display.width}×${display.height})`}
                glyph={<Monitor className="size-4" aria-hidden="true" />}
                selected={isSameTarget(selected, target)}
                onSelect={() => select(target)}
              />
            );
          })}

        {/* Applications next: real icon (or a lucide fallback), name-sorted. */}
        <p className="mt-2 font-medium text-muted-foreground text-xs">{APPLICATIONS_HEADING}</p>
        {applications.length === 0 ? (
          <p className="px-2 text-muted-foreground text-xs">{NO_APPLICATIONS_NOTE}</p>
        ) : (
          applications.map((app) => {
            const target: RecordingTargetVm = {
              kind: "application",
              pid: app.pid,
              bundleId: app.bundleId,
            };
            return (
              <SourceRow
                key={`application-${app.pid}`}
                target={target}
                name={app.name}
                glyph={
                  app.icon !== null ? (
                    <img src={app.icon} alt="" className="size-4" />
                  ) : (
                    <AppWindow className="size-4" aria-hidden="true" />
                  )
                }
                selected={isSameTarget(selected, target)}
                onSelect={() => select(target)}
              />
            );
          })
        )}
      </RadioGroup>

      {/* Inline app-scope disclosure (Story 19.1): shown whenever an application
          is selected — states the exclusion plainly, with the audio clause
          conditioned on the system-audio toggle (Story 19.3). */}
      {selected.kind === "application" && (
        <p className="text-muted-foreground text-xs">
          {appScopeDisclosure(selectedApp?.name ?? "the selected application", systemAudioOn)}
        </p>
      )}

      {/* A selection that vanished from the polled list is marked unavailable —
          never silently swapped; Start against it fails cleanly at the sidecar. */}
      {!selectionAvailable && (
        <p className="text-held text-xs" role="alert">
          {SELECTION_UNAVAILABLE_NOTE}
        </p>
      )}
    </div>
  );
}
