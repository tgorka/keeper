/**
 * The Destination card: the folder chooser (Story 19.5, Epic 19) and the
 * session-folder template (Story 40.2, Epic 40).
 *
 * Rendered by BOTH settings surfaces — the pre-record "Destination" setup card
 * and Settings → Recording — and bound to the one `recording-settings` mirror
 * store, so choosing a folder on either surface persists the same value and
 * both reflect it live. The displayed path is the EFFECTIVE folder resolved by
 * Rust (`~/Movies/keeper` by default), so the UI always names a concrete
 * destination. The chooser opens only the OS-native directory picker already
 * used by the export dialog (`@tauri-apps/plugin-dialog`) — local folders
 * only, zero network affordance. Edits apply to the next Recording Session
 * only; the folder is validated at Start time (exists/writable/free space),
 * never here.
 *
 * The template row names what goes UNDER that folder, and its preview is the
 * entire manual: instead of a token table or a help panel, the card shows the
 * absolute path the next recording would use, recomputed on every keystroke
 * by `recording_path_preview`. Both the clock and the renderer belong to the
 * BACKEND — `keeper-core` is clock-free, and a second renderer in TypeScript
 * would drift from the one that actually names folders — so every sentence
 * here, the path and the fault alike, is Rust-composed and printed verbatim.
 * The field is LOCAL state, never a store binding: a store binding would
 * persist a half-typed template on every keystroke. The next-session title the
 * preview renders against is opt-in (`withNextSessionTitle`), because the card
 * is mounted on two surfaces and only one of them collects a title.
 */
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { useEffect, useId, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { RecordingPathPreviewVm } from "@/lib/ipc/client";
import { recordingPathPreview } from "@/lib/ipc/client";
import { useRecordingMeta } from "@/lib/stores/recording-meta";
import {
  applyRecordingSettings,
  ensureRecordingSettingsHydrated,
  RECORDING_PATH_TEMPLATE_DEFAULT,
  recordingSettingsStore,
  useRecordingSettings,
} from "@/lib/stores/recording-settings";

/** Field label (recording voice: sentence case). */
export const DESTINATION_FOLDER_LABEL = "Folder";

/** The chooser affordance's label (recording voice). */
export const CHOOSE_FOLDER_LABEL = "Choose folder";

/** Honest scope note: edits never mutate a running session (glossary caps). */
export const DESTINATION_NEXT_SESSION_NOTE = "Applies to the next Recording Session.";

/** Honest local-only disclosure — recording adds zero network destinations. */
export const DESTINATION_LOCAL_ONLY_NOTE =
  "Recordings save to this folder on this Mac. Nothing uploads.";

/** Test id for the truncated effective-path display. */
export const DESTINATION_PATH_TESTID = "recording-destination-path";

/** Template field label (recording voice: sentence case). */
export const DESTINATION_TEMPLATE_LABEL = "Session folder";

/** The template's save affordance. The folder row persists the moment the
 * picker confirms; a template cannot, because half a template is not one. */
export const DESTINATION_TEMPLATE_SAVE_LABEL = "Save template";

/** Test id for the raw template field. */
export const DESTINATION_TEMPLATE_TESTID = "recording-template-field";

/** Test id for the template save affordance. */
export const DESTINATION_TEMPLATE_SAVE_TESTID = "recording-template-save";

/** Test id for the Rust-composed preview line (the next session's full path). */
export const DESTINATION_TEMPLATE_PREVIEW_TESTID = "recording-template-preview";

/** Test id for the inline fault line (a Rust-composed refusal, verbatim). */
export const DESTINATION_TEMPLATE_FAULT_TESTID = "recording-template-fault";

export function RecordingDestinationControls({
  withNextSessionTitle = false,
}: {
  /**
   * Preview against the pre-record card's next-session title. Opt-in, and set
   * on that surface only: a title typed for one session must not follow the
   * user into Settings, where the same card previews untitled.
   */
  withNextSessionTitle?: boolean;
}) {
  const settings = useRecordingSettings();
  // Lazy shared hydration: whichever surface mounts first triggers the one
  // read; the other (and any remount) reuses the mirrored value.
  useEffect(() => {
    void ensureRecordingSettingsHydrated();
  }, []);

  // The raw template text. `null` means "not seeded yet": until the store
  // hydrates there is no effective template to show, nothing to preview and
  // nothing to save.
  const [template, setTemplate] = useState<string | null>(null);
  // The same text, readable outside a render: a save that resolves after the
  // user typed again has to tell whether the field still holds what it sent.
  const textRef = useRef<string | null>(null);
  // The last value seeded FROM the store, so one store movement is adopted at
  // most once.
  const seeded = useRef<string | null>(null);
  // Set the moment the user types, cleared when a save is CONFIRMED. An unsaved
  // edit outranks every store movement: this surface's own optimistic mirror
  // update would otherwise retype the field mid-keystroke, and the revert behind
  // a refused write would wipe the very text the refusal is about.
  const edited = useRef(false);
  const [preview, setPreview] = useState<RecordingPathPreviewVm | null>(null);
  // True while the preview for the CURRENT text is still in flight: saving a
  // template nothing has judged yet is how an unparseable one reaches a write.
  const [previewing, setPreviewing] = useState(true);
  // A refusal from the WRITE path (the preview said fine, `recording_settings_set`
  // did not — a race, or its stricter guard). Rust-authored, rendered verbatim.
  const [refusal, setRefusal] = useState<string | null>(null);
  // Monotonic request token (the store's `writeId` pattern): only the newest
  // preview response is applied, so a slow reply that lost to a later keystroke
  // — or to a folder change — can never put an older path back on screen.
  const previewId = useRef(0);
  const templateFieldId = useId();

  // The preview's title is the NEXT-SESSION title, and it belongs to the surface
  // that collects it. `recordingMetaStore` is a module-level singleton whose
  // fields outlive the meta card's mount, so a Settings dialog that read it would
  // preview a title typed on the pre-record pane — a stale title, the one thing
  // that surface must not show, and one that hides `{slug}`'s collapse exactly
  // where the template language is being learned. A trimmed-empty title is sent
  // as `null` so Rust renders that collapse.
  const metaTitle = useRecordingMeta((state) => state.fields.title);
  const title = withNextSessionTitle && metaTitle.trim() !== "" ? metaTitle : null;

  const effectiveTemplate = settings?.pathTemplate ?? null;
  useEffect(() => {
    if (effectiveTemplate === null || effectiveTemplate === seeded.current) {
      return;
    }
    seeded.current = effectiveTemplate;
    if (edited.current) {
      return;
    }
    textRef.current = effectiveTemplate;
    setTemplate(effectiveTemplate);
  }, [effectiveTemplate]);

  // The absolute line is rooted at the EFFECTIVE destination, so choosing a
  // folder above must re-ask for the path below: the command resolves that root
  // itself, but the answer it already gave was rooted at the old one.
  const destinationRoot = settings?.destinationDir ?? null;
  useEffect(() => {
    if (template === null || destinationRoot === null) {
      return;
    }
    previewId.current += 1;
    const id = previewId.current;
    setPreviewing(true);
    // Any refusal on screen described the text, title or folder that just moved.
    setRefusal(null);
    void recordingPathPreview(template, title)
      .then((vm) => {
        if (id === previewId.current) {
          setPreview(vm);
          setPreviewing(false);
        }
      })
      .catch(() => {
        // A failed round trip is not a verdict on the template: show no path,
        // invent no sentence, and leave save disabled until a preview lands.
        if (id === previewId.current) {
          setPreview(null);
          setPreviewing(false);
        }
      });
  }, [template, title, destinationRoot]);

  /** Persist the typed template; print a write-path refusal in the fault slot. */
  const saveTemplate = async (text: string) => {
    // The *live* store value, exactly as `pickFolder` reads it, so the commit
    // cannot clobber a co-setting edited on the sibling surface meanwhile.
    const live = recordingSettingsStore.getState().settings;
    if (live === null) {
      return;
    }
    const refused = await applyRecordingSettings({ ...live, pathTemplate: text });
    const confirmed = recordingSettingsStore.getState().settings?.pathTemplate ?? null;
    seeded.current = confirmed;
    if (refused !== null) {
      // Refused: the mirror reverted to the last confirmed value. `edited` is
      // still set, so that revert cannot re-seed over the text the refusal is
      // about — print the Rust sentence beside it and leave the field to be
      // corrected. No state the preview effect watches moves, so the sentence
      // stays until the user edits.
      setRefusal(refused);
      return;
    }
    // Confirmed: the effective template is authoritative again, and it is not
    // always what was sent — a cleared field comes back as the default. Adopt it,
    // unless the user typed on while the write was in flight, in which case their
    // newer text stands and its own preview is already on the way.
    setRefusal(null);
    if (textRef.current === text && confirmed !== null) {
      edited.current = false;
      textRef.current = confirmed;
      setTemplate(confirmed);
    }
  };

  // Exactly one side of the preview VM is ever populated, and a write-path
  // refusal outranks the clean line the preview left behind.
  const fault = preview?.problem ?? refusal;
  const previewPath = fault === null ? (preview?.absolutePath ?? null) : null;
  // Savable only once a preview for THIS text came back clean, the store has
  // hydrated, and the text is not already the effective template. An empty
  // field IS savable: clearing is a save, and the echoed VM brings the default
  // back (which re-seeds the field).
  const saveDisabled =
    settings === null ||
    template === null ||
    previewing ||
    preview === null ||
    preview.problem !== null ||
    template === settings.pathTemplate;

  /** Open the OS-native directory picker; persist a confirmed selection. */
  const pickFolder = async () => {
    try {
      const selection = await openFolder({ directory: true });
      // Read the *live* store value (not the closed-over render snapshot) so
      // the commit never clobbers a co-setting edited concurrently on the
      // sibling surface while the picker was open.
      const live = recordingSettingsStore.getState().settings;
      if (typeof selection === "string" && live !== null) {
        void applyRecordingSettings({ ...live, destinationDir: selection });
      }
    } catch {
      // Picker cancellation / failure → keep the current folder (no write).
    }
  };

  return (
    <div className="flex flex-col gap-2 text-sm">
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-0.5">
          <Label>{DESTINATION_FOLDER_LABEL}</Label>
          {/* The effective folder is always concrete (Rust resolves the
              default), truncated to the card width with the full path on
              hover; empty only while hydration is still in flight. */}
          <p
            className="truncate font-mono text-muted-foreground text-xs"
            data-testid={DESTINATION_PATH_TESTID}
            title={settings?.destinationDir ?? undefined}
          >
            {settings?.destinationDir ?? ""}
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0"
          disabled={settings === null}
          onClick={() => {
            void pickFolder();
          }}
        >
          {CHOOSE_FOLDER_LABEL}
        </Button>
      </div>
      <div className="flex flex-col gap-1">
        <div className="flex items-center justify-between gap-4">
          <Label htmlFor={templateFieldId}>{DESTINATION_TEMPLATE_LABEL}</Label>
          <div className="flex items-center gap-1">
            <Input
              id={templateFieldId}
              className="w-64 font-mono text-xs"
              data-testid={DESTINATION_TEMPLATE_TESTID}
              // A blank field falls back to the default, so the default is the
              // honest placeholder. The rule lives in Rust; this mirrors the
              // string only.
              placeholder={RECORDING_PATH_TEMPLATE_DEFAULT}
              value={template ?? ""}
              disabled={settings === null}
              onChange={(event) => {
                edited.current = true;
                textRef.current = event.target.value;
                setTemplate(event.target.value);
              }}
            />
            <Button
              type="button"
              variant="outline"
              size="xs"
              className="shrink-0"
              data-testid={DESTINATION_TEMPLATE_SAVE_TESTID}
              disabled={saveDisabled}
              onClick={() => {
                void saveTemplate(template ?? "");
              }}
            >
              {DESTINATION_TEMPLATE_SAVE_LABEL}
            </Button>
          </div>
        </div>
        {/* The path the next session would take, in the mono face — never a
            path this template could not produce, because Rust withholds both
            path fields whenever it has a problem to report instead. */}
        {previewPath !== null && (
          <p
            className="truncate font-mono text-muted-foreground text-xs"
            data-testid={DESTINATION_TEMPLATE_PREVIEW_TESTID}
            title={previewPath}
          >
            {previewPath}
          </p>
        )}
        {/* The Rust-authored reason, verbatim: 40.1 wrote these thirteen
            sentences as inline UI copy, and this is where they print. */}
        {fault !== null && (
          <p className="text-destructive text-xs" data-testid={DESTINATION_TEMPLATE_FAULT_TESTID}>
            {fault}
          </p>
        )}
      </div>
      <p className="text-muted-foreground">{DESTINATION_NEXT_SESSION_NOTE}</p>
      <p className="text-muted-foreground text-xs">{DESTINATION_LOCAL_ONLY_NOTE}</p>
    </div>
  );
}
