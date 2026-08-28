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
 *
 * Story 41.7 adds the one thing a synced folder on a pendrive has to say before
 * Record is pressed: that it is on a drive, and — when the drive is out — that
 * it is not here. A person choosing a pendrive should learn it is a pendrive
 * from the card, not from a failed start. The state is Rust's (`volume::scan`
 * against the volume marker, never an `exists()` on the mountpoint), and a
 * destination that is not on removable media renders none of this copy at all.
 *
 * Story 46.10 makes the card the ONE place the whole path is shown and set. The
 * owner's report was that recordings "automatically drop into a `recordings/`
 * subfolder" with no way to choose it — which was true of this card and false of
 * keeper: the subfolder has always been per-profile and editable, in Settings →
 * Sync → a folder, where nobody configuring a recording would look. So the head
 * appears here, editable, beside the tail it is joined with. They stay TWO
 * fields, deliberately: the head is a field on the sync profile in `sync.db` and
 * has to be identical on every machine syncing the folder, the tail is
 * `recording.path_template` in this machine's settings table and cannot be —
 * merging them would send the second machine somewhere else. The head's write is
 * `sync_profile_save`, the same command the folder form uses, and it is the one
 * control on this card whose consequence has to be read BEFORE the button:
 * changing the head moves no files, so everything already recorded stays under
 * the old one and stops being found.
 */
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useId, useRef, useState } from "react";
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
  RecordingVolumeVm,
} from "@/lib/ipc/client";
import { recordingDestinationProfiles, recordingPathPreview } from "@/lib/ipc/client";
import { useRecordingMeta } from "@/lib/stores/recording-meta";
import {
  applyRecordingSettings,
  ensureRecordingSettingsHydrated,
  RECORDING_PATH_TEMPLATE_DEFAULT,
  recordingSettingsStore,
  refreshRecordingSettings,
  useRecordingSettings,
} from "@/lib/stores/recording-settings";
import {
  SYNC_RECORDINGS_SUBFOLDER_LABEL,
  setSyncProfileRecordingsSubfolder,
  syncErrorMessage,
} from "@/lib/stores/sync";

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

/**
 * What a destination on removable media says for itself (Story 41.7), in the
 * same declarative voice as {@link destinationSyncedNote}: a fact about where
 * the recordings go, not an instruction and not a warning banner.
 *
 * One sentence per state rather than a removable line plus a conditional
 * attached/detached line, because "merope is removable media." followed by
 * "merope isn't attached." is two sentences saying one thing. Each state's
 * sentence carries BOTH facts — that the folder is on a drive, and whether the
 * drive is here.
 *
 * The name is Rust's, read from the volume's own marker. It is `null` when this
 * run has never had that marker in front of it (a drive that was already out at
 * launch), and then the drive is described rather than named — the card does
 * not invent one out of the path.
 */
export function destinationVolumeNote(volume: RecordingVolumeVm): string {
  const subject = volume.name ?? "This folder's drive";
  switch (volume.state) {
    case "attached":
      return `${subject} is removable media, so this folder is only here while the drive is plugged in.`;
    case "absent":
      return `${subject} isn't attached, so a recording can't start until you plug it in.`;
    case "unexpected":
      return `${subject} isn't attached — a different volume is mounted in its place, so a recording can't start.`;
  }
}

/** Test id for the two-way destination choice (Story 41.2). */
export const DESTINATION_CHOICE_TESTID = "recording-destination-choice";

/** Test id for the synced-folder picker's trigger. */
export const DESTINATION_PROFILE_SELECT_TESTID = "recording-destination-profile";

/** Test id for the consequence sentence a synced destination prints. */
export const DESTINATION_SYNCED_NOTE_TESTID = "recording-destination-synced-note";

/** Test id for the removable-media sentence (Story 41.7). */
export const DESTINATION_VOLUME_NOTE_TESTID = "recording-destination-volume-note";

/**
 * The head's save affordance, worded like the template's beside it: the folder
 * row persists the moment the picker confirms, and neither of the two text
 * fields can, because half a path is not one.
 */
export const DESTINATION_SUBFOLDER_SAVE_LABEL = "Save subfolder";

/** Test id for the head's save affordance. */
export const DESTINATION_SUBFOLDER_SAVE_TESTID = "recording-destination-subfolder-save";

/** Test id for the head's own refusal line (Rust-authored, printed verbatim). */
export const DESTINATION_SUBFOLDER_FAULT_TESTID = "recording-destination-subfolder-fault";

/** Test id for the sentence that says the head travels and where it lives. */
export const DESTINATION_SUBFOLDER_TRAVELS_TESTID = "recording-destination-subfolder-travels";

/** Test id for the consequence a head change carries, printed BEFORE the write. */
export const DESTINATION_SUBFOLDER_CONSEQUENCE_TESTID =
  "recording-destination-subfolder-consequence";

/** Test id for the sentence that says the tail does NOT travel. */
export const DESTINATION_TEMPLATE_LOCAL_TESTID = "recording-template-local-note";

/** Last-resort message when a head write is rejected with no readable sentence. */
export const DESTINATION_SUBFOLDER_UNKNOWN_ERROR =
  "keeper could not change this folder's recordings subfolder.";

/**
 * Where the head LIVES, which is the whole reason it is not merged into the
 * template beside it (Story 46.10).
 *
 * The head is a field on the sync profile in `sync.db`; the tail is
 * `recording.path_template` in this machine's settings table. Merging them would
 * put a fact that must be identical on every machine syncing the folder into a
 * key that cannot be, and the second machine would record somewhere else. So the
 * card shows both and says which is which, in the same declarative voice as
 * {@link destinationSyncedNote}: a fact about the setting, not a warning.
 */
export function destinationSubfolderTravelsNote(profileName: string): string {
  return `This is part of ${profileName} itself, so every machine syncing it records into the same subfolder. More than one folder deep is fine — 40-media/recordings.`;
}

/**
 * The other half of that pair: the template is this machine's alone.
 *
 * Only shown when a synced folder is the destination. With a plain folder
 * nothing about the destination travels, so naming one half as local would
 * imply the other half is not.
 */
export const DESTINATION_TEMPLATE_LOCAL_NOTE =
  "This half is this Mac's alone. Other machines syncing the same folder keep their own.";

/**
 * What changing the head COSTS, said before the write and not after (Story
 * 46.10) — the one sentence in this card that has to be read before a button is
 * pressed rather than after.
 *
 * Changing the subfolder is a pure configuration write: `parse_req` moves the
 * string and `RecordingsConfig` is a flag plus a path, so nothing under the old
 * head is copied, moved or rewritten (`docs/recording.md` says the same about
 * unflagging). The bytes are safe and everything that POINTS at them is not:
 * the recordings archive is rebuilt by walking the recordings root, so sessions
 * under the old head leave the browser at the next rebuild, and a session's note
 * stub embeds `![[<head>/…]]`, which stops resolving the moment the head moves.
 *
 * The old head is interpolated because "sessions already there" is not a place a
 * person can go and look at, and the whole point of the sentence is that they
 * can.
 */
export function destinationSubfolderConsequence(storedSubfolder: string): string {
  return `Saving this moves no files. Sessions already in ${storedSubfolder} stay exactly where they are — they drop out of the recordings browser at the next rebuild, and the ![[…]] links in their notes stop resolving. Move them yourself first if you want to keep them listed.`;
}

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

  // The synced folders this destination may be pointed at. Read on mount, and
  // re-read after a head write, which changes both the subfolder these rows carry
  // and the root they resolve to. Empty is the honest resting state: no flagged
  // profile, no engine and no git all resolve the same empty list, and all three
  // mean "today's card".
  const [profiles, setProfiles] = useState<RecordingProfileVm[]>([]);
  // Guards the state write rather than the request, so a reload triggered by a
  // save that resolves after the card closed is dropped instead of warning.
  const alive = useRef(true);
  useEffect(
    () => () => {
      alive.current = false;
    },
    [],
  );
  const loadProfiles = useCallback(async () => {
    try {
      const list = await recordingDestinationProfiles();
      if (!alive.current) {
        return;
      }
      setProfiles((current) =>
        // An empty answer over an empty list is already the resting state, and
        // adopting it would schedule a render only to say nothing changed — on the
        // machine with no flagged profile, which is every machine until one is
        // flagged. Over a NON-empty list it is news: a folder unflagged elsewhere
        // has to leave the picker.
        list.length === 0 && current.length === 0 ? current : list,
      );
    } catch {
      // The command already answers `[]` for every "sync cannot say"; a failed
      // round trip is the same answer, and inventing a picker over it would
      // offer folders nothing can confirm exist.
    }
  }, []);
  useEffect(() => {
    void loadProfiles();
  }, [loadProfiles]);
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
  // Story 41.7: present only when the destination is a synced folder on removable
  // media. Read straight off the settings VM, which Rust re-scans on every read,
  // so the sentence follows the drive in and out of the port with no other
  // action here.
  const volume: RecordingVolumeVm | null = settings?.destinationVolume ?? null;
  // A picker needs something to pick. Rust degrades an unusable profile to the
  // folder answer, so a synced kind with no list means the list itself could not
  // be read — and an empty select with no way out is worse than the plain
  // chooser, which is at least an escape. The COPY still follows the kind: the
  // recordings are wherever they are, whatever this card can offer.
  const showPicker = syncedChoice && profiles.length > 0;

  // --- The HEAD: the per-profile half of the path (Story 46.10) -------------
  //
  // The owner's ask was "let me pick the whole subfolder path where I set the
  // session folder". The whole path is two settings with two different lifetimes:
  // this head is a field on the sync profile in `sync.db` and travels to every
  // machine syncing the folder; the template below is `recording.path_template`
  // in this machine's settings table and does not. They are shown together and
  // labelled apart — merging them would put a fact that must be identical on both
  // machines into a key that cannot be.
  //
  // The row in force, or `null`: with no synced destination there is no head to
  // show, and with a synced kind whose row did not come back there is no folder
  // behind an edit box either.
  const headProfile =
    syncedChoice && settings?.destinationProfileId != null
      ? (profiles.find((profile) => profile.id === settings.destinationProfileId) ?? null)
      : null;
  // The stored head, as Rust composed `destinationDir` from it. Never sliced back
  // out of that path: the join normalises nothing, so `20-media//sessions` and
  // `20-media/sessions` resolve to one root and are two different stored values,
  // and only the stored one may be echoed back to a profile write.
  const headProfileId = headProfile?.id ?? null;
  const storedHead = headProfile?.subfolder ?? null;
  // The raw head text, on the template field's terms exactly: local state, never a
  // store binding, because a binding would persist half a subfolder per keystroke.
  const [head, setHead] = useState<string | null>(null);
  const headRef = useRef<string | null>(null);
  const headSeeded = useRef<string | null>(null);
  const headEdited = useRef(false);
  // Which folder the box is currently about. `undefined` is "no answer yet", which
  // is not the same as the `null` a plain-folder destination gives.
  const headFolder = useRef<string | null | undefined>(undefined);
  const [headRefusal, setHeadRefusal] = useState<string | null>(null);
  const [headSaving, setHeadSaving] = useState(false);
  const headFieldId = useId();
  useEffect(() => {
    if (headProfileId !== headFolder.current) {
      // A DIFFERENT folder is a different setting, so whatever was typed for the
      // previous one is not an unsaved edit of this one — it is text about a
      // folder that is no longer on screen, and keeping it would offer to write
      // one folder's subfolder onto another.
      headFolder.current = headProfileId;
      headEdited.current = false;
      headSeeded.current = storedHead;
      headRef.current = storedHead;
      setHead(storedHead);
      setHeadRefusal(null);
      return;
    }
    if (storedHead === null || storedHead === headSeeded.current) {
      return;
    }
    headSeeded.current = storedHead;
    if (headEdited.current) {
      return;
    }
    headRef.current = storedHead;
    setHead(storedHead);
  }, [headProfileId, storedHead]);

  /**
   * Write the head onto the profile, then re-read everything it moved.
   *
   * The write is `sync_profile_save` — the same command the folder form in
   * Settings → Sync uses, reached through the one helper that re-expresses a
   * stored profile faithfully. There is no second writer for this field, and a
   * refusal is `RecordingsConfig::validate`'s own sentence, printed verbatim.
   */
  const saveHead = async (text: string) => {
    if (headProfile === null || headSaving) {
      return;
    }
    setHeadSaving(true);
    setHeadRefusal(null);
    try {
      await setSyncProfileRecordingsSubfolder(headProfile.id, text);
      // Both of this card's path lines were composed against the OLD head: the
      // resolved root lives on the settings VM (which Rust joins from the profile)
      // and the head itself lives on the picker rows. Re-read both, or the card
      // keeps printing the path it just stopped using.
      await Promise.all([refreshRecordingSettings(), loadProfiles()]);
      // Confirmed: let the seeding effect adopt whatever Rust stored, which is not
      // always what was sent — it trims. Unless the user typed on meanwhile, in
      // which case their newer text stands.
      if (headRef.current === text) {
        headEdited.current = false;
      }
    } catch (raw) {
      // Rust-authored, beside the field it is about: "must not be empty", "must
      // be relative to the profile folder", "must not escape the profile folder",
      // "overlaps notes subfolder …". `headEdited` stays set, so the re-read above
      // cannot retype the box over the text the refusal describes.
      setHeadRefusal(syncErrorMessage(raw, DESTINATION_SUBFOLDER_UNKNOWN_ERROR));
    } finally {
      setHeadSaving(false);
    }
  };

  // Savable once there is a folder, a box, no write in flight, and the box says
  // something other than what is stored. An EMPTY box is savable: clearing it is a
  // deliberate act and Rust refuses it in its own words, which is more useful than
  // a greyed button that explains nothing.
  const headSaveDisabled =
    headProfile === null || head === null || headSaving || head.trim() === storedHead;
  // The consequence, on screen from the first keystroke that makes the box differ
  // and until the write lands — so it is read on the way to the button, not after
  // it. Gated on the same condition as the button being live, because that IS the
  // condition "a head change is pending".
  const headPending = storedHead !== null && head !== null && head.trim() !== storedHead;

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
      {/* The HEAD (Story 46.10): the per-profile half of the path, under the
          folder it belongs to and above the template it is joined with, so the
          card reads in the order the path is built. Present only for a synced
          destination with a row behind it — a plain folder has no profile, and a
          box over a folder nothing confirmed exists would be a dead control.
          Named with the same words Settings → Sync names it, from one constant:
          it is one setting, and the person who found it there has to recognise
          it here. */}
      {headProfile !== null && (
        <div className="flex flex-col gap-1">
          <div className="flex items-center justify-between gap-4">
            <Label htmlFor={headFieldId}>{SYNC_RECORDINGS_SUBFOLDER_LABEL}</Label>
            <div className="flex items-center gap-1">
              <Input
                id={headFieldId}
                className="w-64 font-mono text-xs"
                value={head ?? ""}
                disabled={settings === null || headSaving}
                onChange={(event) => {
                  headEdited.current = true;
                  headRef.current = event.target.value;
                  setHead(event.target.value);
                  // The refusal on screen judged the text that just moved.
                  setHeadRefusal(null);
                }}
              />
              <Button
                type="button"
                variant="outline"
                size="xs"
                className="shrink-0"
                data-testid={DESTINATION_SUBFOLDER_SAVE_TESTID}
                disabled={headSaveDisabled}
                onClick={() => {
                  void saveHead(head ?? "");
                }}
              >
                {DESTINATION_SUBFOLDER_SAVE_LABEL}
              </Button>
            </div>
          </div>
          {/* Which of the two halves this is, and that it travels — the whole
              reason it is not merged into the template below. The profile's
              display name comes off the settings VM where Rust resolved it, and
              falls back to the picker row's own name for the window where the
              settings read has not caught up with a rename. */}
          <p
            className="text-muted-foreground text-xs"
            data-testid={DESTINATION_SUBFOLDER_TRAVELS_TESTID}
          >
            {destinationSubfolderTravelsNote(profileName ?? headProfile.name)}
          </p>
          {/* BEFORE the write, never after it: on screen from the first keystroke
              that makes the box differ from what is stored, and gone again the
              moment the write lands or the box is typed back. Un-muted, because
              this is the one line on the card that has to be read on the way to a
              button rather than after it — and not `text-destructive`, because
              nothing here is refused or wrong. */}
          {headPending && storedHead !== null && (
            <p
              className="text-foreground text-xs"
              data-testid={DESTINATION_SUBFOLDER_CONSEQUENCE_TESTID}
            >
              {destinationSubfolderConsequence(storedHead)}
            </p>
          )}
          {/* `RecordingsConfig::validate`'s own sentence, verbatim and beside the
              field it judged — its own slot rather than the template's, because a
              profile write and a settings write are two different verdicts and
              collapsing them would print one under the wrong box. */}
          {headRefusal !== null && (
            <p
              className="text-destructive text-xs"
              data-testid={DESTINATION_SUBFOLDER_FAULT_TESTID}
            >
              {headRefusal}
            </p>
          )}
        </div>
      )}
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
        {/* The other half of the pair the head above names (Story 46.10). Only
            when a synced folder is the destination: with a plain folder nothing
            about the destination travels, so calling this half local would imply
            the other half is not. */}
        {headProfile !== null && (
          <p
            className="text-muted-foreground text-xs"
            data-testid={DESTINATION_TEMPLATE_LOCAL_TESTID}
          >
            {DESTINATION_TEMPLATE_LOCAL_NOTE}
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
      {/* The drive, when there is one. Its own line under the consequence
          sentence rather than folded into it: whether recordings get pushed and
          whether the folder is here right now are two different facts, and only
          the second one changes when someone pulls a stick out. A destination on
          an ordinary disk renders nothing here at all. */}
      {syncedChoice && volume !== null && (
        <p className="text-muted-foreground text-xs" data-testid={DESTINATION_VOLUME_NOTE_TESTID}>
          {destinationVolumeNote(volume)}
        </p>
      )}
    </div>
  );
}
