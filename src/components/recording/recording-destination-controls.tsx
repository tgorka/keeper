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
 *
 * Story 41.2 turns the destination from a path into a DECISION: "a folder" or
 * "a synced folder". Epic 41's position is that sync is a consequence of where
 * recordings live rather than a checkbox beside it (UX-DR47), so there is no
 * second "also sync my recordings" toggle here — choosing a synced folder IS
 * the choice, and the card states what it means. The two-way choice and the
 * profile picker appear only when `recording_destination_profiles` returns a
 * recordings-flagged profile; with none (or with no git on the machine, which
 * resolves the same empty list) this is exactly the card it was before. The
 * resolved absolute root is composed in ONE place — Rust — and arrives as
 * `destinationDir` whichever kind is in force, so no line here ever joins a
 * profile's local path to a subfolder.
 */
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { useEffect, useId, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type {
  RecordingDestinationKind,
  RecordingPathPreviewVm,
  RecordingProfileVm,
  RecordingSettingsVm,
} from "@/lib/ipc/client";
import { recordingDestinationProfiles, recordingPathPreview } from "@/lib/ipc/client";
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

/** Honest local-only disclosure for the PLAIN folder choice — recording adds
 * zero network destinations of its own. It is replaced (never merely joined)
 * by {@link destinationSyncedNote} when a synced folder is the destination,
 * because "Nothing uploads." would then be a lie the card is telling about a
 * consequence it just arranged. */
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

/** The two-way choice's group label (Story 41.2, recording voice). */
export const DESTINATION_CHOICE_LABEL = "Where recordings live";

/** The plain-folder choice (recording voice: an article, not a command). */
export const DESTINATION_CHOICE_FOLDER_LABEL = "A folder";

/** The synced-folder choice. "Synced folder" is the glossary term for a sync
 * profile's tree, so the choice names the thing the user already knows. */
export const DESTINATION_CHOICE_PROFILE_LABEL = "A synced folder";

/** Row label once the synced-folder choice is in force (recording voice). */
export const DESTINATION_SYNCED_FOLDER_LABEL = "Synced folder";

/**
 * The consequence of a synced destination, stated rather than toggled: this
 * card arranges no sync of its own, it puts recordings somewhere a profile is
 * already responsible for — so the sentence names that profile as the actor.
 * The resolved root is NOT interpolated here; it stays on its own line, the
 * one Rust composed.
 */
export function destinationSyncedNote(profileName: string): string {
  return `Recordings save here on this Mac, and ${profileName} commits and pushes them.`;
}

/** Test id for the two-way destination choice (Story 41.2). */
export const DESTINATION_CHOICE_TESTID = "recording-destination-choice";

/** Test id for the synced-folder picker's trigger. */
export const DESTINATION_PROFILE_SELECT_TESTID = "recording-destination-profile";

/** Test id for the consequence sentence a synced destination prints. */
export const DESTINATION_SYNCED_NOTE_TESTID = "recording-destination-synced-note";

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

  // The synced folders this destination may be pointed at, read ONCE per mount.
  // Empty is the honest resting state: no flagged profile, no engine and no git
  // all resolve the same empty list, and all three mean "today's card".
  const [profiles, setProfiles] = useState<RecordingProfileVm[]>([]);
  useEffect(() => {
    let mounted = true;
    void recordingDestinationProfiles()
      .then((list) => {
        // An empty answer is already the resting state, and adopting it would
        // schedule a render only to say nothing changed — on the machine with
        // no flagged profile, which is every machine until one is flagged.
        if (mounted && list.length > 0) {
          setProfiles(list);
        }
      })
      .catch(() => {
        // The command already answers `[]` for every "sync cannot say"; a failed
        // round trip is the same answer, and inventing a picker over it would
        // offer folders nothing can confirm exist.
      });
    return () => {
      mounted = false;
    };
  }, []);
  // A refusal from a DESTINATION write (an unflagged profile, an ambiguous
  // folder). Kept apart from the template refusal above because the preview
  // effect clears that one whenever the resolved root moves — and the optimistic
  // update, then the revert behind the refusal, move it twice. Rust-authored,
  // printed verbatim in the same fault slot.
  const [destinationRefusal, setDestinationRefusal] = useState<string | null>(null);
  const folderChoiceId = useId();
  const profileChoiceId = useId();

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

  // Exactly one side of the preview VM is ever populated, and a template
  // write-path refusal outranks the clean line the preview left behind.
  const templateFault = preview?.problem ?? refusal;
  const previewPath = templateFault === null ? (preview?.absolutePath ?? null) : null;
  // One fault slot, two sources. A destination refusal is the newest verdict the
  // user asked for, so it outranks — and it does NOT suppress the preview line,
  // which is about the template and stays true while the folder row is wrong.
  const fault = destinationRefusal ?? templateFault;
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

  /**
   * Persist a destination decision through the shared mirror and print a
   * refusal, verbatim, in the fault slot. The refused write reverts the mirror,
   * so the choice on screen falls back to the one that is actually in force —
   * the card never shows a decision the database declined.
   */
  const commitDestination = async (patch: Partial<RecordingSettingsVm>) => {
    // Read the *live* store value (not the closed-over render snapshot) so the
    // commit never clobbers a co-setting edited concurrently on the sibling
    // surface while the picker was open.
    const live = recordingSettingsStore.getState().settings;
    if (live === null) {
      return;
    }
    // Clear synchronously, before the round trip: the verdict on screen judged
    // the previous decision, and this one supersedes it either way.
    setDestinationRefusal(null);
    const refused = await applyRecordingSettings({ ...live, ...patch });
    setDestinationRefusal(refused);
  };

  /** Open the OS-native directory picker; persist a confirmed selection. */
  const pickFolder = async () => {
    try {
      const selection = await openFolder({ directory: true });
      if (typeof selection === "string") {
        // A named folder is also the answer to "a folder or a synced folder?",
        // and it is the way OUT of a synced folder: Rust reads `destinationDir`
        // as the input only under `kind: "folder"`.
        await commitDestination({
          destinationKind: "folder",
          destinationProfileId: null,
          destinationDir: selection,
        });
      }
    } catch {
      // Picker cancellation / failure → keep the current folder (no write).
    }
  };

  /** Point the destination at a specific synced folder. */
  const chooseProfile = async (id: string) => {
    const live = recordingSettingsStore.getState().settings;
    if (live === null || (live.destinationKind === "profile" && id === live.destinationProfileId)) {
      return;
    }
    // `destinationDir` is an OUTPUT under this kind — Rust resolves the root
    // from the id, which is why a profile rename never strands a stale string.
    await commitDestination({ destinationKind: "profile", destinationProfileId: id });
  };

  /** Switch between the plain-folder and synced-folder answers. */
  const chooseKind = async (value: string) => {
    const live = recordingSettingsStore.getState().settings;
    if (live === null || value === live.destinationKind) {
      return;
    }
    if (value === "folder") {
      // "A folder" means a folder the user names, so the choice opens the
      // picker instead of submitting anything on its own. The tempting shortcut
      // — send a BLANK `destinationDir` and let Rust clear the key — is wrong
      // here in both directions: blank means "no opinion", and on a machine
      // with exactly one flagged profile the no-opinion answer resolves back to
      // THAT profile, so the choice would silently do nothing on precisely the
      // machine where it matters most. Echoing the profile's resolved root back
      // is no better: it IS that profile's recordings root, the spec's
      // "unambiguous exception", which Rust normalises straight back. A real
      // path is the one submission that always means what it says. Cancelling
      // writes nothing and the radio falls back to the decision still in force
      // — a folder nobody named is not a decision.
      await pickFolder();
      return;
    }
    // The radio only exists when a flagged profile does, so there is always one
    // to fall back on; a previously chosen id is kept if it is still offered.
    const chosen =
      profiles.find((profile) => profile.id === live.destinationProfileId) ?? profiles[0];
    if (chosen !== undefined) {
      await chooseProfile(chosen.id);
    }
  };

  // The decision in force. `folder` is the answer before hydration too, so the
  // card's resting shape is the plain one.
  const kind: RecordingDestinationKind = settings?.destinationKind ?? "folder";
  const syncedChoice = kind === "profile";
  const profileName = settings?.destinationProfileName ?? null;
  // A picker needs something to pick. Rust degrades an unusable profile to the
  // folder answer, so a synced kind with no list means the list itself could not
  // be read — and an empty select with no way out is worse than the plain
  // chooser, which is at least an escape. The COPY still follows the kind: the
  // recordings are wherever they are, whatever this card can offer.
  const showPicker = syncedChoice && profiles.length > 0;

  return (
    <div className="flex flex-col gap-2 text-sm">
      {/* The choice exists only when there is a second answer to give. With no
          recordings-flagged profile — and with no engine or no git, which
          resolve the same empty list — there is no radio, no picker and no new
          copy: this is the card it has always been. */}
      {profiles.length > 0 && (
        <RadioGroup
          className="gap-1"
          value={kind}
          onValueChange={(value) => {
            void chooseKind(value);
          }}
          disabled={settings === null}
          aria-label={DESTINATION_CHOICE_LABEL}
          data-testid={DESTINATION_CHOICE_TESTID}
        >
          <div className="flex items-center gap-2">
            <RadioGroupItem value="folder" id={folderChoiceId} />
            <Label htmlFor={folderChoiceId}>{DESTINATION_CHOICE_FOLDER_LABEL}</Label>
          </div>
          <div className="flex items-center gap-2">
            <RadioGroupItem value="profile" id={profileChoiceId} />
            <Label htmlFor={profileChoiceId}>{DESTINATION_CHOICE_PROFILE_LABEL}</Label>
          </div>
        </RadioGroup>
      )}
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-0.5">
          <Label>{syncedChoice ? DESTINATION_SYNCED_FOLDER_LABEL : DESTINATION_FOLDER_LABEL}</Label>
          {/* ONE line of truth for both choices: the effective root, always
              concrete (Rust resolves the default, and resolves a profile's
              recordings root from its id — nothing is joined here), truncated
              to the card width with the full path on hover; empty only while
              hydration is still in flight. */}
          <p
            className="truncate font-mono text-muted-foreground text-xs"
            data-testid={DESTINATION_PATH_TESTID}
            title={settings?.destinationDir ?? undefined}
          >
            {settings?.destinationDir ?? ""}
          </p>
        </div>
        {showPicker ? (
          <Select
            value={settings?.destinationProfileId ?? undefined}
            onValueChange={(id) => {
              void chooseProfile(id);
            }}
            disabled={settings === null}
          >
            <SelectTrigger
              className="w-48 shrink-0"
              data-testid={DESTINATION_PROFILE_SELECT_TESTID}
              aria-label={DESTINATION_CHOICE_PROFILE_LABEL}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {profiles.map((profile) => (
                <SelectItem key={profile.id} value={profile.id}>
                  {profile.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : (
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
        )}
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
                // The two features share one fault slot. A destination refusal
                // outranks the template's verdict, so it has to yield the
                // moment the template is the thing being judged again.
                setDestinationRefusal(null);
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
            sentences as inline UI copy, and 41.2's two destination refusals
            join them here. */}
        {fault !== null && (
          <p className="text-destructive text-xs" data-testid={DESTINATION_TEMPLATE_FAULT_TESTID}>
            {fault}
          </p>
        )}
      </div>
      <p className="text-muted-foreground">{DESTINATION_NEXT_SESSION_NOTE}</p>
      {/* The same slot, one honest sentence. A synced destination cannot be
          described as "Nothing uploads.", and it is not a toggle that needs
          confirming either — it is a consequence of the folder just chosen, so
          the card simply says who acts on it. Silent (never the local-only
          claim) in the degenerate case where the kind says profile but no name
          came back, because the one thing worse than no sentence is a false
          one. */}
      {syncedChoice ? (
        profileName !== null && (
          <p className="text-muted-foreground text-xs" data-testid={DESTINATION_SYNCED_NOTE_TESTID}>
            {destinationSyncedNote(profileName)}
          </p>
        )
      ) : (
        <p className="text-muted-foreground text-xs">{DESTINATION_LOCAL_ONLY_NOTE}</p>
      )}
    </div>
  );
}
