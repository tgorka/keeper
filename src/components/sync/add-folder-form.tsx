/**
 * The add-and-edit folder form (Epic 29 Story 29.5, Epic 32 Story 32.7, Epic 33
 * Story 33.4).
 *
 * One component rendered in two places (AD-C7) — Settings → Sync, and the Sync
 * view — and in two modes: given a {@link SyncProfileVm} it edits that profile,
 * without one it creates a folder. Two copies would drift, and an edit-only
 * second copy would drift hardest, because this form carries rules that are not
 * obvious from a field list:
 *   - The `worktree` lane is an agent airlock Rust accepts only together with
 *     `pushOnly`, so the form never offers it and always sends
 *     {@link SYNC_DEFAULT_LANE}. A second copy could easily grow a lane control
 *     that composes a profile Rust rejects.
 *   - The three numeric knobs are seconds/MB here and milliseconds/bytes on the
 *     wire, and an empty field means "keeper picks" rather than zero. That is
 *     NOT `null` on the wire: `null` is the omission Rust reads as "leave
 *     whatever is stored" (AD-34-9), so an empty field sends keeper's documented
 *     default, which is the value `effective_settle_ms` reads as "nothing
 *     pinned". Each box's placeholder names the number that will be in force,
 *     and a note appears under it when the typed number is not that number
 *     (AD-34-8).
 *   - The recordings subfolder is the one field whose empty box means two
 *     different things (Story 41.7). Adding: nobody has answered, so the field
 *     is omitted and `RecordingsConfig`'s own default stands — this form holds
 *     no copy of it, and takes the value to prefill from
 *     `SyncProfileVm.recordingsSubfolder`, which Rust resolves for a folder that
 *     holds no recordings yet. Editing: the box arrived holding the value in
 *     force, so an empty one is a deliberate clear and goes as an empty string
 *     for the shared validator to refuse in its own words. Nothing here
 *     re-implements those rules or tidies input up to make a save succeed.
 *   - The access token is a second write to a different store (the OS
 *     keychain), keyed by the profile id. Its failure is reported as its own
 *     thing, because the profile is stored by then. An edit form reads it back
 *     as the form opens and shows it masked (Story 34.12, which overrides
 *     AD-34-7 at the owner's instruction), so the field holds a *copy* of what
 *     the keychain holds and a save has to compare the two.
 *   - Which keychain call a save makes is therefore decided by
 *     {@link credentialWrite} against the answer the form opened with, never by
 *     the field alone: an empty box is what a removed token, a folder that
 *     never had one, and a read that failed all look like, and only the first
 *     of those may delete anything.
 *   - `SyncProfileReq` carries no `enabled`, and Rust merges an update onto a
 *     CLONE of the stored profile rather than rebuilding it, so saving an edit
 *     leaves a paused folder paused and cannot move any field this form does not
 *     show. An edit must therefore never be routed through anything that also
 *     toggles pause.
 *   - The local path of an EXISTING profile is not editable. The engine binds a
 *     profile to its folder — and on removable media to a marker written inside
 *     it — so repointing a profile is not an edit to it but a different folder.
 *     While adding, the path is both picked and typeable, and a leading `~` is
 *     resolved here rather than in Rust ({@link syncExpandHome} says why).
 *
 * The heading is deliberately not part of this component: each surface titles
 * it in its own chrome — Settings with a section heading, the Sync view with
 * the disclosure button that revealed it. The `<form>` carries
 * {@link SYNC_ADD_TITLE}, or {@link SYNC_EDIT_TITLE} and the folder's name, as
 * its accessible name instead, so it is named for a screen reader wherever it
 * is rendered, including where nothing visible repeats the title.
 */
import { homeDir } from "@tauri-apps/api/path";
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronRight, Eye, EyeOff } from "lucide-react";
import { type FormEvent, useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import type { SyncProfileVm } from "@/lib/ipc/client";
// The credential calls are made straight from the form rather than through the
// mirror store: none of them change anything the store mirrors, and the read
// belongs to one open of one form rather than to state worth keeping in sync.
// The notes flag is the exception — it DOES change what the vault mirror holds,
// so it is followed by a refresh of that mirror.
import { syncClearCredential, syncGetCredential, syncSetCredential } from "@/lib/ipc/client";
import {
  ensureNotesVaultsHydrated,
  notesVaultsStore,
  refreshNoteVaults,
} from "@/lib/stores/notes-vaults";
import {
  SYNC_DEFAULT_BRANCH,
  SYNC_DEFAULT_LANE,
  SYNC_DEFAULT_LFS_THRESHOLD_BYTES,
  SYNC_DEFAULT_POLL_INTERVAL_MS,
  SYNC_DEFAULT_RELEASE_TTL_MS,
  SYNC_DEFAULT_SETTLE_MS,
  SYNC_DIRECTIONS,
  SYNC_LFS_MODES,
  SYNC_MIN_POLL_INTERVAL_MS,
  SYNC_MIN_RELEASE_TTL_MS,
  SYNC_RECORDINGS_SUBFOLDER_LABEL,
  SYNC_RELEASE_TTL_CEILING_MS,
  SYNC_REMOVABLE_SETTLE_MS,
  type SyncDirection,
  type SyncLfsMode,
  saveSyncProfile,
  syncErrorMessage,
} from "@/lib/stores/sync";
import { refreshSyncDetail } from "@/lib/stores/sync-detail";
import { cn } from "@/lib/utils";

/**
 * The form's title. Doubles as the label of the Sync view's header action,
 * because the action and the thing it reveals must not be worded differently.
 */
export const SYNC_ADD_TITLE = "Add a folder";

/**
 * The edit form's title, and the label of every per-folder action that reveals
 * it — the action and the thing it reveals must not be worded differently. The
 * form's accessible name appends the folder's own name, because several of
 * them can be open at once.
 */
export const SYNC_EDIT_TITLE = "Edit folder";

/** Field labels. */
export const SYNC_NAME_LABEL = "Name";
export const SYNC_FOLDER_LABEL = "Folder";
export const SYNC_CHOOSE_FOLDER_LABEL = "Choose folder";
export const SYNC_NO_FOLDER_CHOSEN_LABEL = "No folder chosen";

/**
 * The Home affordance beside the picker, and the box's placeholder (Story
 * 59.8).
 *
 * The placeholder is the only documentation `~` gets: a box that silently
 * accepts a tilde teaches nobody that it does, and a note under this row would
 * be a fourth line under a field that already carries three.
 */
export const SYNC_HOME_FOLDER_LABEL = "Home";
export const SYNC_FOLDER_PLACEHOLDER = "~/notes";

/**
 * What syncing a home directory pulls in, said once, where the choice is made
 * (Story 59.8, the epic's own acceptance for it).
 *
 * The owner asked for "option of home dir" and this is the half of that ask
 * that is not a button. Home is a legal folder — `SyncProfile::validate`
 * refuses nothing about it and this form must not either — but it is the one
 * choice on this screen whose consequences are invisible until the first sync
 * has already pushed them: a home directory is not a folder of documents, it is
 * every application's private state plus the keys that unlock the remote this
 * folder is about to be pushed to.
 *
 * Named things rather than a category, because "large and sensitive files" is
 * something everybody agrees with and nobody acts on, and each name here is one
 * a person can go and look at. It points at the box that solves it rather than
 * at documentation: the decision is being made in this form, so its remedy has
 * to be in this form.
 */
export const SYNC_HOME_FOLDER_WARNING =
  "This is your whole home folder, so keeper would sync everything under it: every app's caches and databases, ~/Library or ~/.config, your ~/.ssh and ~/.gnupg keys, every node_modules and build directory, every virtual-machine image — tens of gigabytes that change constantly, and secrets that would be pushed to the remote along with them. A folder inside home is almost always the folder you mean. If you do mean this one, fill in Skip these files under Advanced options before you add it.";

/**
 * What the folder box means, with a leading `~` resolved against this machine's
 * home directory (Story 59.8).
 *
 * **Why here and not in Rust, which is the whole of this story's second half.**
 * The invariant worth protecting is that a literal `~` can never reach the
 * store as a directory name — and that invariant is already held, in Rust, by
 * something this file cannot weaken: `SyncProfile::validate` refuses a
 * `local_path` that is not absolute (`keeper-sync/src/profile/mod.rs:1158`),
 * `~/notes` is not absolute, and every entrance goes through it — this form's
 * IPC save, `keeper-syncd profile add`, a hand-edited `config.toml`, and
 * `db::upsert_profile`, which validates before the row is written whatever
 * route it arrived by. So expansion is not the guard. The guard exists, and the
 * worst case of an expansion that does not happen is a refusal quoting the path
 * rather than a folder called `~`.
 *
 * What is left is a convenience, and it belongs to the layer where the home
 * directory is a fact about the *person*: this app runs as them, so `homeDir()`
 * is their home. Expanding inside `keeper-sync` would expand against the `HOME`
 * of whichever process happened to perform the write, and `keeper-syncd` can
 * run as a different user from the one who edited its config — so the same `~`
 * in the same file would mean different folders depending on who saved it. An
 * expansion whose meaning depends on the writer is worse than no expansion,
 * because nothing about it looks wrong. (The daemon's own `platform.rs:189`
 * home lookup is not a precedent for this: it resolves the home of the process
 * for the process's own data directory, which is exactly the question that has
 * one right answer there and no right answer here.)
 *
 * It also costs nothing extra: the form has to know where home is anyway, for
 * the Home control and for the warning, so this is one fact used three times
 * rather than a second source of truth.
 *
 * **`~` and `~/…` only.** `~alice/notes` is left exactly as typed: this form
 * cannot know another user's home directory, and expanding it to *this* user's
 * would silently sync a different folder from the one that was named. Left
 * alone it is not absolute, so it comes back from Rust as "local path must be
 * absolute, got ~alice/notes" — a refusal naming the path, which is the honest
 * answer to a path this layer cannot resolve.
 *
 * `home === null` leaves the text alone for the same reason and it is the case
 * that matters most: the home directory is a fact about the machine, read from
 * the shell, and a browser that could not read it must not fall back to a
 * guess. `/home/$USER` is wrong on macOS, `$HOME` is not readable from a
 * webview, and either would be wrong in the one direction that cannot be
 * noticed — a plausible path that is not this person's home.
 *
 * **Nothing else is normalized.** No `..` collapsing, no trailing-slash
 * trimming, no symlink resolution: those are the engine's business and a second
 * opinion about them here would be a second answer to disagree with. This
 * function does exactly one substitution.
 */
export function syncExpandHome(raw: string, home: string | null): string {
  if (home === null || (raw !== "~" && !raw.startsWith("~/"))) {
    return raw;
  }
  const rest = raw.slice(2);
  return rest === "" ? home : `${home}/${rest}`;
}

/**
 * Why an existing profile's folder is shown but not offered. Explaining beats
 * dropping the field: a path that quietly vanished in edit mode would read as
 * one the form forgot rather than one the engine holds fixed.
 */
export const SYNC_PATH_FIXED_NOTE =
  "keeper binds a folder to this profile when it is set up — on removable storage, to a marker written inside it — so the path stays fixed. Syncing a different folder means adding that one separately.";
export const SYNC_REMOTE_URL_LABEL = "Remote URL";
export const SYNC_BRANCH_LABEL = "Branch";
export const SYNC_DIRECTION_LABEL = "Direction";
export const SYNC_ADVANCED_LABEL = "Advanced options";
export const SYNC_LFS_MODE_LABEL = "Large files";
export const SYNC_LFS_THRESHOLD_LABEL = "Track files at or above (MB)";
export const SYNC_REMOVABLE_LABEL = "This folder is on removable or network storage";
export const SYNC_REMOVABLE_NOTE =
  "keeper marks the drive and follows it, so unplugging pauses this folder instead of syncing " +
  "everything on it as deleted. It also waits longer for writes to settle, because those volumes " +
  "report changes late. The folder has to be on the drive, not on this computer's own disk.";

/**
 * The advanced knobs (Story 32.7, AD-S8; Story 34.5, AD-34-8). Every one of
 * these was already carried by `SyncProfileReq` and hardcoded away by this form.
 *
 * Each numeric knob shows the number that is actually IN FORCE. Leaving one
 * empty means "keeper picks", and the placeholder then names what keeper will
 * pick; typing a number pins it, and if Rust would not honour that number
 * verbatim the note under the field says what it will use instead. A blank box
 * that silently meant 5 000 was the whole of AD-34-8.
 */
export const SYNC_SETTLE_LABEL = "Wait for writes to stop (seconds)";
export const SYNC_SETTLE_NOTE =
  "How long a file must go untouched before keeper copies it. Left empty, keeper picks the wait itself — and picks a longer one on removable or network storage, where changes are reported late.";
export const SYNC_POLL_LABEL = "Re-read the folder every (seconds)";
export const SYNC_POLL_NOTE =
  "How often keeper walks the whole folder looking for changes. Lower notices a change sooner and costs more on a drive or a network share. Left empty, keeper picks the interval itself.";
export const SYNC_SUBJECT_LABEL = "Commit subject";
export const SYNC_SUBJECT_NOTE =
  "The first line of every commit keeper makes for this folder. {profile} is the folder's name; {added}, {modified}, {deleted} and {changed} are file counts. Left empty, keeper writes its own — sync(folder): 3 added, 1 modified.";
export const SYNC_EXCLUDES_LABEL = "Skip these files";
export const SYNC_EXCLUDES_NOTE =
  "Comma-separated patterns, for example *.tmp, .DS_Store. Left empty, keeper syncs everything in the folder.";

/**
 * The virtual-file controls (Story 56.12, Epic 56, AD-122, AD-132).
 *
 * Epic 56 shipped the whole machinery for leaving a large file's content on the
 * server and fetching it when somebody asks — and no way to say which files, or
 * for how long. These three are that surface. They live under Advanced beside
 * the two LFS knobs because they are about the same files: a path only has
 * content to leave behind if keeper routed it through LFS in the first place.
 *
 * The wording avoids "virtual", which is jargon here, and says what happens to
 * the file instead.
 */
export const SYNC_VIRTUAL_PATTERNS_LABEL = "Files that may stay away";
export const SYNC_VIRTUAL_PATTERNS_NOTE =
  "Comma-separated patterns, for example scans/**, *.psd. keeper keeps a matched file as a placeholder and fetches its content when you open it. Left empty, the folder's own committed .keepervirtual file decides — that list travels with the folder, so everyone syncing it gets the same answer — and with no such file either, the size below decides on its own.";
export const SYNC_VIRTUAL_OVER_LABEL = "Only files at or above (MB)";
export const SYNC_RELEASE_TTL_LABEL = "Give local copies back after (hours)";
export const SYNC_RELEASE_TTL_NOTE =
  "How long a file keeps its content on this machine after keeper has confirmed that content reached the server. Then keeper drops back to the placeholder and fetches the file again when you ask for it. 0 never gives anything back.";

/** The line under the release box when it says never. */
export const SYNC_RELEASE_NEVER_NOTE =
  "keeper will not release anything in this folder: every file stays here once its content arrives.";

/**
 * The four lines under the SIZE-FLOOR box, of which exactly one is on screen
 * and true (stories 56.14 and 56.16).
 *
 * The box carried ONE unconditional note — "A matched file smaller than this is
 * downloaded anyway" — a sentence about a match, under a control that in the
 * owner's own configuration matched nothing at all. He saved
 * `virtualPatterns: []` with a 1 MiB floor, which can only mean "don't fetch the
 * big files", and the form talked to him about matched files while all 16 GB
 * downloaded. Rust now reads a floor with no permissive line in force as the
 * selector, so each state the pair of boxes can be in gets its own sentence,
 * chosen by {@link syncVirtualOverNote} from the predicates the save itself
 * uses.
 *
 * Four states and not three because the patterns box holds a LIST and Rust
 * splits every list in half: a `!` line protects, everything else authorizes,
 * and only the authorizing half can take the decision away from the floor
 * (story 56.14). A box holding nothing but protections is therefore in the
 * WIDEST state rather than the narrowest, which is the one reading a sentence
 * chosen by list length gets exactly backwards.
 *
 * This one is the line when the box's content means "no floor" (story 56.14).
 *
 * The box's coercion was silent, and `0` is not a neutral fallback here — it is
 * the documented instruction that nothing stays away for being large, which
 * anything {@link syncVirtualOverBytes} rounds to zero lands on: a blank box, a
 * typed `0`, a minus sign, a half-typed `1e`. Meanwhile the release box beside
 * it explains BOTH of its coercions, so two adjacent boxes with the same failure
 * shape behaved differently for no reason a reader could see.
 *
 * Worded as the consequence rather than as the parse, because the person reading
 * it is looking at their own files and did not type a number they thought was
 * invalid. 56.16 changed the consequence and so the words: with no floor, a file
 * stays away only if a list names it, and the widest reading of the old
 * sentence — "every matched file may stay away" — was a promise about matches
 * under a form where there might be none.
 *
 * It names no single control as the one that can do the naming, and that is a
 * correction rather than vagueness. The sentence said "a pattern above names
 * it", meaning the box directly overhead — but with that box empty, which is
 * the state this note is most often read in, the thing deciding is the
 * committed `.keepervirtual`, exactly as {@link SYNC_VIRTUAL_PATTERNS_NOTE} one
 * control higher says in the same breath. Two adjacent notes contradicted each
 * other in the default state of every folder the form seeds, and a person whose
 * files are absent because of a policy that travelled with the repository was
 * pointed at an empty box as the only possible cause.
 */
export const SYNC_VIRTUAL_OVER_NONE_NOTE =
  "No size limit, so nothing stays away just for being large: only the list in force — this box, or the folder's own committed .keepervirtual — decides, whatever a file's size.";

/**
 * The line when the floor is the only thing selecting anything (story 56.16).
 *
 * This is the owner's state, and the one the form had no sentence for. It names
 * the committed `.keepervirtual` because that file outranks this box and can
 * narrow the floor's reach to a list — a person told the size "decides on its
 * own" would otherwise be surprised by a folder that obeys its own file.
 *
 * "every file keeper tracks" and not "every file". The floor only ever reaches
 * a path that already holds LFS pointer text, and `lfs::stage` routes a file
 * through LFS only at or above the threshold box above. With the threshold at
 * 10 MB and this box at 1, every file in between is a plain git blob that can
 * never become a placeholder, and the unqualified promise was false for the
 * whole band. The qualifier ties the claim to the control that decides it;
 * {@link SYNC_VIRTUAL_OVER_BELOW_LFS_NOTE} says the rest out loud.
 */
export const SYNC_VIRTUAL_OVER_ALONE_NOTE =
  "Nothing is named above, so this size decides on its own: every file keeper tracks that is at least this big stays away as a placeholder until you open it, unless the folder ships its own .keepervirtual naming a narrower list.";

/**
 * The line for a box holding only protections beside a real floor (story 56.16).
 *
 * The state {@link syncVirtualPositivePatterns} exists to find: entries in the
 * box, none of them authorizing anything. Rust unions the protections into
 * whatever zone is in force and leaves the floor selecting, so this reads as
 * "the floor decides, minus those" — the opposite of the matched line, which is
 * what a length test rendered here.
 *
 * An entry Rust drops as punctuation — a bare `!`, a lone `/` — lands under
 * this sentence too, and the first clause then overstates a typo. The clause
 * that matters survives it: a dropped entry names nothing, so "except the ones
 * those entries name" excepts nothing and the floor really does decide alone.
 */
export const SYNC_VIRTUAL_OVER_PROTECTED_ONLY_NOTE =
  "Every entry above starts with !, so each one protects a file rather than choosing one: this size decides on its own, and every file keeper tracks that is at least this big stays away except the ones those entries name.";

/**
 * The line when patterns select and the floor merely holds the small ones back
 * — the field's original job, and now the only state that sentence is true in.
 */
export const SYNC_VIRTUAL_OVER_MATCHED_NOTE =
  "Of the files named above, only those at least this big stay away; a smaller one is downloaded anyway, because fetching it later costs more than keeping it.";

/**
 * The two lines for how the floor sits against the LFS threshold (story 56.16),
 * of which {@link syncVirtualOverBandNote} shows at most one.
 *
 * Both are gated on `lfsMode === "materialize"` because that is the only mode
 * either sentence describes: it is the mode that both routes a file through LFS
 * and keeps its content on this machine. `pointerOnly` keeps no content to begin
 * with and `disabled` tracks nothing through LFS at all, so under either of
 * those these would describe a state the folder cannot be in — and a note that
 * claims a band a mode does not have is the same class of lie as the one this
 * story fixes.
 *
 * This one is the ordering where the floor is the higher of the two: a file
 * between them is uploaded and also kept.
 */
export const SYNC_VIRTUAL_OVER_ABOVE_LFS_NOTE =
  "A file between the two sizes — big enough for keeper to track, too small to stay away — is uploaded to the server and also kept on this computer.";

/**
 * The other ordering, and the one that was missing (story 56.16).
 *
 * Only the ordering above had a line, which is the ordering in which the floor's
 * own sentence is already true. In the inverse one the floor is below the
 * threshold and reaches nothing under it, so the floor's promise stops short of
 * where it appears to reach — the box says 1 MB and the tracking size says 10,
 * and the 9 MB in between are plain git blobs kept forever. That is the ordering
 * a person lands in by lowering this box to make MORE stay away, so it is the
 * one that needed saying.
 */
export const SYNC_VIRTUAL_OVER_BELOW_LFS_NOTE =
  "This size is below the tracking size above, so the tracking size is what decides: a file smaller than that is stored in git and always kept on this computer, whatever this box says.";

/** The line under the release box when Rust will refuse the window outright. */
export const SYNC_RELEASE_OUT_OF_RANGE_NOTE =
  "keeper refuses a window under a minute or over ten years rather than rounding it, so this save will come back with an error. Use 0 to switch releasing off.";

/**
 * What a control wears when the folder's own config file decides its value
 * (Story 56.12, AD-132).
 *
 * Two idioms already exist for this and they disagree, on purpose.
 * `FileControlled` in Settings **says so and deliberately does not disable**,
 * because `set_setting` still writes the settings table underneath the file and
 * the value a person types there takes effect the moment the file stops setting
 * the key — disabling would make an honest fallback unreachable. Every clause
 * of that argument inverts here. `profile::as_stored` does not lose a race with
 * a file; it STRIPS the key out of the row on every write and restores the
 * previous value, reporting it with a `tracing::warn!` nobody sees. A control
 * that accepted the edit would report success and revert. So this one follows
 * the form's other "you cannot change this here" shape instead —
 * {@link SYNC_PATH_FIXED_NOTE}, a plain note under a control that is not
 * offered — and the save omits the key rather than sending a value that would
 * be thrown away.
 */
export function syncFolderOwnedNote(profileKey: string): string {
  return `${profileKey} is set by this folder's own config file (.keeper/keeper.toml). keeper keeps that value, so this cannot be changed here.`;
}

export const SYNC_SUBPATHS_LABEL = "Sync only these folders";
export const SYNC_SUBPATHS_NOTE =
  "Comma-separated paths inside the folder. Left empty, keeper syncs the whole folder.";
export const SYNC_TAGS_LABEL = "Tags";
export const SYNC_TAGS_NOTE =
  "Comma-separated. Each tag is written into the message of every commit keeper makes for this folder.";
export const SYNC_AUTHOR_LABEL = "Commit author";
export const SYNC_AUTHOR_NOTE =
  "Name <email>, an address, or just a name. Left empty, commits are authored by this device.";

/**
 * The notes-vault control (Epic 35, Story 37.1, FR-94, FR-120, FR-121, AD-54).
 *
 * This switch IS the whole of "make this a notes vault": a vault is not a
 * configured object, it is a synced folder carrying a notes config, so flagging
 * is the only configuration a vault requires. There is no vault picker, no path
 * field and no import flow anywhere in the product, and the absence is the
 * design — reintroducing one would be a regression.
 *
 * The subfolder field appears only once the switch is on, because a subfolder
 * for a folder that is not a vault is a question about nothing. It is prefilled
 * with keeper's real default rather than left empty, so the common case is
 * already answered and the resolved path can be shown underneath it.
 */
export const SYNC_NOTES_LABEL = "This folder is a notes vault";
export const SYNC_NOTES_NOTE =
  "keeper keeps markdown notes in a subfolder of this folder and syncs them with everything else here. Obsidian reads the same files unchanged.";
export const SYNC_NOTES_SUBFOLDER_LABEL = "Notes subfolder";

/** keeper's default vault subfolder — mirrors `NotesConfig::default_subfolder`. */
export const SYNC_NOTES_DEFAULT_SUBFOLDER = "notes";

/**
 * The three promises the card makes about what keeper will and will not touch
 * (FR-121), rendered as lines rather than as a link: a docs link for a claim
 * about someone's own files is a claim they have to go and check.
 */
export const SYNC_NOTES_GUARANTEES = [
  "`.obsidian/` is never read or written",
  "`.keeper/` holds the index cache and is added to this folder's ignore rules, so it never syncs",
  "keeper never moves a file you did not ask it to move",
] as const;

/**
 * The recordings control (Epic 41, Story 41.7, AD-66).
 *
 * The notes-vault control directly above, applied a second time and deliberately
 * not reinvented: same place in the form, same switch-and-subfolder shape, same
 * kind of sentence. A second idiom for "this folder also holds X" would make the
 * third one ambiguous, and this pair is the whole vocabulary the Sync form has
 * for it.
 *
 * This is the switch that was missing. `RecordingsConfig` shipped in Story 41.1
 * and the Recording pane's destination picker in 41.2, and that picker renders
 * only when a profile carries a recordings block — so with nothing in the app
 * able to write one, both were unreachable and the Recording pane offered a
 * plain folder and nothing else. Reported from the field on a machine holding
 * two synced folders, both `recordings: null`, one of them already a notes vault
 * flagged from this very form. That asymmetry was the bug.
 *
 * Note what is NOT here: a default subfolder. `SYNC_NOTES_DEFAULT_SUBFOLDER`
 * above is a second spelling of a Rust constant, one rename away from telling
 * the user a vault lives somewhere it does not. The recordings default comes off
 * `SyncProfileVm.recordingsSubfolder`, which Rust resolves to the stored value
 * or to `RecordingsConfig`'s own default — see {@link formValuesFor}.
 */
export const SYNC_RECORDINGS_LABEL = "This folder holds recordings";
export const SYNC_RECORDINGS_NOTE =
  "keeper saves recordings into a subfolder of this folder and syncs them with everything else here. A folder flagged this way can be chosen as the destination in Recording.";

/**
 * Shown only while adding a folder, where no stored profile has resolved the
 * subfolder yet and the box therefore starts empty. Worded like the advanced
 * knobs below ("Left empty, keeper picks the wait itself") because it is the
 * same promise: keeper's own default, not a value this form invented. On an edit
 * form the box arrives filled in, so emptying it is a deliberate act — and the
 * shared validator refuses it in its own words rather than quietly restoring
 * what was there.
 */
export const SYNC_RECORDINGS_SUBFOLDER_NOTE = "Left empty, keeper picks the subfolder itself.";

/**
 * The sessions control (Phase 7, FR-222, AD-107).
 *
 * The recordings control directly above, applied a third time and deliberately
 * not reinvented: same place in the form, same switch-and-subfolder shape, same
 * kind of sentence — the established vocabulary for "this folder also holds X".
 *
 * As with recordings, no default subfolder is spelled here: it comes off
 * `SyncProfileVm.sessionsSubfolder`, which Rust resolves to the stored value or
 * to `SessionsConfig`'s own default (`60-sessions`) — see {@link formValuesFor}.
 */
export const SYNC_SESSIONS_LABEL = "This folder has sessions";
export const SYNC_SESSIONS_NOTE =
  "keeper lists LLM work sessions from a subfolder of this folder — one folder per session, with a README, promoted artifacts and reusable prompts. keeper adopts the layout that is there; it does not create one.";
export const SYNC_SESSIONS_SUBFOLDER_LABEL = "Sessions subfolder";
export const SYNC_SESSIONS_SUBFOLDER_NOTE = "Left empty, keeper picks the subfolder itself.";

/**
 * The access-token field (Story 32.7, AD-S7; Story 34.4, AD-34-7; Story 34.12,
 * which overrides AD-34-7). The token is written to the OS keychain in a second
 * call once the profile has an id, and an edit form reads it back as it opens
 * and shows it masked. So the note under the field has to say which of the
 * three answers the keychain gave — a token, none, or a read that did not
 * complete — because two of them leave the field looking identical while
 * meaning opposite things at save time.
 */
export const SYNC_TOKEN_LABEL = "Access token";
export const SYNC_TOKEN_NOTE =
  "Used to authenticate with the remote. keeper stores it in the system keychain when the folder is added, and fills this field in again when you edit the folder.";
export const SYNC_TOKEN_READING_NOTE = "Reading the stored token from the system keychain.";
export const SYNC_TOKEN_EDIT_NOTE =
  "This is the token stored in the system keychain, shown as dots until you press the eye. Type a different one to replace it, or empty the field to remove it from the keychain.";
export const SYNC_TOKEN_NONE_STORED_NOTE =
  "No token is stored for this folder. Type one to store it in the system keychain.";

/**
 * What the field means while the form does not know what is stored. Saying it
 * outright matters more than naming the failure: the field is empty, emptying
 * it is how a token is removed, and the user has to be told that this
 * particular empty field will remove nothing.
 */
export const SYNC_TOKEN_UNREADABLE_NOTE =
  "keeper could not read the stored token, so saving leaves whatever is there as it is and an empty field will not remove it. Type a new one to replace it.";
export const SYNC_TOKEN_READ_FAILED_PREFIX = "The stored token could not be read: ";

/**
 * The eye toggle over the field, named for what pressing it will do rather than
 * for the state it is in, because that is what a screen reader announces before
 * the press.
 */
export const SYNC_TOKEN_SHOW_LABEL = "Show token";
export const SYNC_TOKEN_HIDE_LABEL = "Hide token";

/**
 * Reported when the profile was stored but the keychain was not. Two writes,
 * two outcomes: "add failed" would send the user back to a form that can now
 * only reject as a duplicate, and "save failed" would send them back to redo
 * changes the engine already took. A removal gets its own wording, because
 * "not stored" describes the opposite of what was attempted.
 */
export const SYNC_TOKEN_FAILED_PREFIX = "The folder was added, but the token was not stored: ";
export const SYNC_TOKEN_EDIT_FAILED_PREFIX =
  "The changes were saved, but the token was not stored: ";
export const SYNC_TOKEN_REMOVE_FAILED_PREFIX =
  "The changes were saved, but the token was not removed: ";

/** The submit, worded for what it does, and the way out of a revealed form. */
export const SYNC_ADD_SUBMIT_LABEL = "Add folder";
export const SYNC_EDIT_SUBMIT_LABEL = "Save changes";
export const SYNC_FORM_CANCEL_LABEL = "Cancel";

/**
 * The line under a numeric knob whose box holds a number Rust will not use.
 *
 * Reachable two ways: a scan cadence below the floor keeper can work with, and
 * a wait of exactly the default on removable storage — which Rust reads as "no
 * wait pinned" and answers with the longer removable window. Either way the box
 * would otherwise show a number that is not the number in force, which is the
 * defect AD-34-8 names.
 */
export function syncInForceNote(seconds: number): string {
  return `keeper is using ${seconds} s here.`;
}

/**
 * The same line for the one knob measured in hours (Story 56.12).
 *
 * A sibling rather than a unit parameter on {@link syncInForceNote}: that
 * function's sentence is asserted verbatim by the tests of two other controls,
 * and widening its signature to serve a third would make every one of those
 * assertions depend on a call this file makes elsewhere. Two short sentences
 * cost less than that coupling.
 */
export function syncReleaseInForceNote(hours: number): string {
  return `keeper is using ${hours} h here.`;
}

/** Test id for the form's chosen-folder display (a read-only truncated path). */
export const SYNC_FORM_PATH_TESTID = "sync-form-path";

/** Test id for the Advanced disclosure toggle. */
export const SYNC_ADVANCED_TOGGLE_TESTID = "sync-advanced-toggle";

/** Test id for the direction Select trigger. */
export const SYNC_DIRECTION_SELECT_TESTID = "sync-direction-select";

/** Test id for the LFS-mode Select trigger. */
export const SYNC_LFS_SELECT_TESTID = "sync-lfs-select";

/** Honest labels for the three directions (the values mirror Rust). */
const DIRECTION_LABELS: Record<SyncDirection, string> = {
  bidirectional: "Both ways",
  pushOnly: "Push only",
  pullOnly: "Pull only",
};

/** Honest labels for the three LFS modes (the values mirror Rust). */
const LFS_MODE_LABELS: Record<SyncLfsMode, string> = {
  materialize: "Download large files",
  pointerOnly: "Keep pointers only",
  disabled: "Do not use LFS",
};

/** Milliseconds in an hour — the unit the release box is typed in. */
const MS_PER_HOUR = 60 * 60 * 1000;

/** The form's fields, all as typed. */
interface SyncFormValues {
  name: string;
  localPath: string;
  remoteUrl: string;
  branch: string;
  direction: SyncDirection;
  lfsMode: SyncLfsMode;
  /** The LFS threshold in MB; empty defers to the Rust default. */
  lfsThresholdMb: string;
  /** Comma-separated patterns whose content may stay away; empty is silence. */
  virtualPatterns: string;
  /** The size floor in MB below which a matched file is fetched anyway. */
  virtualOverMb: string;
  /**
   * The retention window in hours. `0` is keeper's documented "never release"
   * and is a real answer here, not the "keeper picks" an empty box means on the
   * two windows below — which is exactly why this one is not parsed by
   * {@link pinnedValue}.
   */
  releaseHours: string;
  removable: boolean;
  /**
   * The settle window in seconds; empty means the profile pins none and keeper
   * picks — a different thing from pinning keeper's own number, and the
   * distinction is what `effective_settle_ms` reads (AD-34-8).
   */
  settleSeconds: string;
  /** The scan cadence in seconds; empty means keeper picks, same rule. */
  pollSeconds: string;
  /** Comma-separated patterns; empty excludes nothing. */
  excludes: string;
  /** Comma-separated paths inside the folder; empty syncs all of it. */
  subpaths: string;
  /** Comma-separated commit tags; empty adds no tag trailers. */
  tags: string;
  /** The commit-subject template; empty means keeper's mechanical subject. */
  commitSubjectTemplate: string;
  /** The commit-author override; empty keeps the device identity. */
  authorOverride: string;
  /**
   * The access token. Typed from empty on an add, and seeded from the keychain
   * when an edit form opens (Story 34.12) — so on an edit form this is a copy
   * of a stored secret, and emptying it is how that secret is removed.
   */
  token: string;
  /**
   * Whether this folder holds a notes vault (FR-94). Unlike every other field
   * here it is not part of `SyncProfileReq`: flagging is a second write, to
   * `notes_vault_flag`, keyed by the profile id — so like the access token it
   * can only happen once the profile has one.
   */
  notesVault: boolean;
  /** Where inside the folder the vault lives; only meaningful when flagged. */
  notesSubfolder: string;
  /**
   * Whether this folder holds recordings (Story 41.7, AD-66). Unlike the notes
   * pair above it IS part of `SyncProfileReq`, so it rides the profile save and
   * needs no second write and no profile id first.
   */
  recordings: boolean;
  /**
   * Where inside the folder recordings live; only meaningful when flagged.
   *
   * Empty means two different things by mode, and both are what the owner meant.
   * On an add form the box starts empty and nobody has answered yet, so the save
   * omits the field and Rust's own default stands. On an edit form the box
   * arrives holding the subfolder in force, so emptying it is deliberate — the
   * save sends the empty string and `RecordingsConfig::validate` refuses it by
   * name, which is the answer rather than an obstacle to route around.
   */
  recordingsSubfolder: string;
  /**
   * Whether this folder holds a sessions zone (FR-222, AD-107). Like the
   * recordings pair above it IS part of `SyncProfileReq`, so it rides the
   * profile save and needs no second write and no profile id first.
   */
  sessions: boolean;
  /**
   * Where inside the folder the sessions zone lives; only meaningful when
   * flagged. Empty follows the recordings rule exactly: unanswered on an add
   * (keeper's default stands), a deliberate clear on an edit (refused by name).
   */
  sessionsSubfolder: string;
}

const EMPTY_FORM: SyncFormValues = {
  name: "",
  localPath: "",
  remoteUrl: "",
  branch: SYNC_DEFAULT_BRANCH,
  direction: "bidirectional",
  lfsMode: "materialize",
  lfsThresholdMb: String(SYNC_DEFAULT_LFS_THRESHOLD_BYTES / 1024 / 1024),
  virtualPatterns: "",
  // Both hold keeper's own number rather than being left blank, the way the
  // threshold above does and unlike the two "keeper picks" windows below: there
  // is no unpinned state for either. `0` IS the no-floor answer and 24 h IS the
  // retention window, so an empty box would be a third state neither field has.
  virtualOverMb: "0",
  releaseHours: String(SYNC_DEFAULT_RELEASE_TTL_MS / MS_PER_HOUR),
  removable: false,
  settleSeconds: "",
  pollSeconds: "",
  excludes: "",
  subpaths: "",
  tags: "",
  commitSubjectTemplate: "",
  authorOverride: "",
  token: "",
  notesVault: false,
  notesSubfolder: SYNC_NOTES_DEFAULT_SUBFOLDER,
  recordings: false,
  // No default spelled here: keeper owns it, and an add form has no stored
  // profile to have resolved it yet. The empty box is how this form says
  // "keeper picks", exactly as the two numeric knobs above do.
  recordingsSubfolder: "",
  sessions: false,
  // Empty for the recordings reason directly above.
  sessionsSubfolder: "",
};

/**
 * A stored profile as the fields that carry it. Everything editable starts from
 * what is stored; the token starts empty even here, because a profile carries
 * none. It lives in the keychain, and arrives — if it arrives — from the read
 * the form starts as it opens.
 */
function formValuesFor(profile: SyncProfileVm): SyncFormValues {
  return {
    name: profile.name,
    localPath: profile.localPath,
    remoteUrl: profile.remoteUrl,
    branch: profile.branch,
    // Both enums are closed mirrors of a Rust match that rejects anything
    // else, so the fallback is unreachable in a build whose UI and engine ship
    // together — it exists because `SyncProfileVm` widens them to `string`.
    direction: SYNC_DIRECTIONS.find((legal) => legal === profile.direction) ?? "bidirectional",
    lfsMode: SYNC_LFS_MODES.find((legal) => legal === profile.lfsMode) ?? "materialize",
    // Back into the units these fields are typed in. `null` on the two windows
    // means the profile pins nothing, and an empty box is how the form says that
    // back — never a number, which the next save would store as a choice the
    // user never made.
    lfsThresholdMb: String(profile.lfsThresholdBytes / 1024 / 1024),
    // The three virtualization knobs, seeded from what is IN FORCE: the VM's
    // values already carry the folder's own config layer, so a folder file that
    // sets one of these is what the box shows — and `folderOwned` is what stops
    // the box from pretending the number is editable.
    virtualPatterns: profile.virtualPatterns.join(", "),
    virtualOverMb: String(profile.virtualOverBytes / 1024 / 1024),
    releaseHours: String(profile.releaseTtlMs / MS_PER_HOUR),
    settleSeconds: profile.settleMs === null ? "" : String(profile.settleMs / 1000),
    pollSeconds: profile.pollIntervalMs === null ? "" : String(profile.pollIntervalMs / 1000),
    removable: profile.removable,
    excludes: profile.excludes.join(", "),
    subpaths: profile.subpaths.join(", "),
    tags: profile.tags.join(", "),
    commitSubjectTemplate: profile.commitSubjectTemplate,
    authorOverride: profile.authorOverride ?? "",
    token: "",
    // Both are seeded from the vault mirror once it has been read, not from the
    // profile: `SyncProfileVm` carries no notes field, and `NoteVaultVm` is
    // where "this folder is a vault, and here is its subfolder" actually lives.
    notesVault: false,
    notesSubfolder: SYNC_NOTES_DEFAULT_SUBFOLDER,
    // Straight off the profile, unlike the notes pair above: `recordings` IS a
    // field on `SyncProfile`, so the VM carries both the flag and the subfolder
    // that would be in force — the stored one, or `RecordingsConfig`'s default
    // for a folder that holds none yet (AD-34-8). That resolved value is why
    // there is no `SYNC_RECORDINGS_DEFAULT_SUBFOLDER` beside the notes one.
    recordings: profile.recordings,
    recordingsSubfolder: profile.recordingsSubfolder,
    // Straight off the profile, exactly as recordings above: the VM carries the
    // flag and the subfolder that would be in force (AD-34-8).
    sessions: profile.sessions,
    sessionsSubfolder: profile.sessionsSubfolder,
  };
}

/**
 * The number a numeric box holds, or `null` when it holds nothing usable.
 *
 * Empty, unparseable and non-positive all mean the same thing to this form —
 * "keeper picks" — because a zero-second wait would commit half-written files, a
 * zero-second cadence would re-read the tree on every tick, and a zero-byte LFS
 * threshold would route every file through LFS.
 */
function pinnedValue(raw: string): number | null {
  const parsed = Number.parseFloat(raw);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

/**
 * The retention window the release box means, in milliseconds.
 *
 * Deliberately not {@link pinnedValue}, and this is the one numeric box in the
 * form that could not reuse it. That helper collapses everything not `> 0` to
 * `null`, which is right where "nothing" means "keeper picks" — and wrong here,
 * because `0` is keeper's documented instruction to switch the release sweep
 * off, and it switches it off *before* the due clock is read so that turning it
 * back on later does not fire a window that armed while it was off. Routed
 * through `pinnedValue`, "never release" would be unreachable from this form.
 *
 * A blank or unparseable box is the 24 h default rather than an omission: there
 * is no unpinned state for this field, so `null` on the wire would mean "this
 * form has no such control", which is false the moment the box is on screen.
 * A negative number — which `min={0}` already refuses in the browser — reads as
 * the default too, rather than being sent for Rust to refuse a second time.
 *
 * A non-zero window under {@link SYNC_MIN_RELEASE_TTL_MS} is NOT rounded up
 * here. Rust refuses it by name and prints its own sentence beside the form,
 * which is the answer; correcting it here would save a number nobody typed.
 *
 * The two guards below the parse exist because the *rounding* can change the
 * instruction rather than merely blur it. Anything under half a millisecond of
 * an hour rounds to `0`, which is not a small window — it is keeper's documented
 * "never release", the opposite of what a person typing a tiny number asked for.
 * And a number large enough to leave the safe-integer range serializes as
 * `Infinity` → `null`, which the wire reads as "not expressed", so the save
 * would report success and change nothing. Both are answered by handing Rust a
 * value it will refuse by name, which is the outcome the person can act on.
 */
function releaseTtlMsFor(raw: string): number {
  const hours = Number.parseFloat(raw);
  if (!Number.isFinite(hours) || hours < 0) {
    return SYNC_DEFAULT_RELEASE_TTL_MS;
  }
  const ms = Math.round(hours * MS_PER_HOUR);
  if (!Number.isSafeInteger(ms)) {
    return SYNC_RELEASE_TTL_CEILING_MS + 1;
  }
  return hours > 0 && ms === 0 ? 1 : ms;
}

/**
 * The wait keeper will actually use, given what the form holds right now.
 *
 * Mirrors `SyncProfile::effective_settle_ms`, including the part that surprises:
 * a wait of exactly keeper's own default IS how "keeper picks" is stored, so a
 * removable folder gets the longer window whether the box is empty or holds a 5.
 * Recomputed from the LIVE form rather than read off `effectiveSettleMs`,
 * because ticking the removable box changes the answer before anything is saved.
 *
 * No ceiling clamp: Rust refuses a window above `SETTLE_CEILING_MS` outright, so
 * a value this would clamp cannot be saved in the first place.
 */
function effectiveSettleSeconds(form: SyncFormValues): number {
  const pinned = pinnedValue(form.settleSeconds);
  const ms = pinned === null ? SYNC_DEFAULT_SETTLE_MS : pinned * 1000;
  const effective = form.removable && ms === SYNC_DEFAULT_SETTLE_MS ? SYNC_REMOVABLE_SETTLE_MS : ms;
  return effective / 1000;
}

/** The scan cadence keeper will actually use — mirrors the same-named Rust fn. */
function effectivePollSeconds(form: SyncFormValues): number {
  const pinned = pinnedValue(form.pollSeconds);
  const ms = pinned === null ? SYNC_DEFAULT_POLL_INTERVAL_MS : pinned * 1000;
  return Math.max(ms, SYNC_MIN_POLL_INTERVAL_MS) / 1000;
}

/**
 * Split one comma-separated advanced field into the string list Rust expects,
 * dropping blanks so a trailing comma or a stray space never reaches the engine
 * as an empty pattern that matches everything.
 */
function splitSyncList(raw: string): string[] {
  return raw
    .split(",")
    .map((entry) => entry.trim())
    .filter((entry) => entry !== "");
}

/**
 * The permissive half of what the patterns box holds — a mirror of Rust's
 * `Parsed::of` and `anchor_line` (stories 56.14 and 56.16).
 *
 * `keeper-sync/src/lfs/virtual_policy.rs` is the authority and this is a
 * deliberate copy of its reading. Rust splits every policy source in two: a `!`
 * line PROTECTS a path and everything under it, anything else AUTHORIZES one to
 * stay away, and only the authorizing half decides whether a list is in force
 * at all — a box of nothing but protections overrides no committed list and
 * leaves the floor selecting on its own, with those entries unioned in as
 * exceptions.
 *
 * The duplication buys the one thing these notes exist for. Deciding the
 * sentence on the list's LENGTH — which is what this replaces — put
 * `!30-masters` beside a 1 MiB floor under "Of the files named above, only
 * those at least this big stay away", when Rust reads that same row as the
 * WIDEST state there is: the floor selects, every tracked file at or above it
 * stays away, and the named zone is the single exception. Both halves of the
 * sentence were inverted, on a row the edit form seeds straight out of
 * `profile.virtualPatterns`, and which is verbatim the fixture of Rust's own
 * `a_profile_protection_still_wins_over_a_floor_that_selects_on_its_own`. The
 * alternative to a mirror here is not less code; it is a sentence that
 * contradicts the engine underneath it.
 *
 * Three rules, each one Rust's:
 *
 * * `\!` and `\#` escape a filename that really begins with `!` or `#`, so an
 *   escaped entry is POSITIVE — without the rule those two filenames would be
 *   unnameable.
 * * An unescaped leading `!` is a protection and never authorizes.
 * * `anchor_line` answers nothing for punctuation with no pattern behind it —
 *   a bare `!`, `/` or `!/` — and the entry is dropped whole rather than
 *   counted as the source having spoken. It strips ONE leading and ONE
 *   trailing `/`, which is why the check below does the same instead of
 *   stripping every slash.
 *
 * `#` is NOT a comment in this box. The profile list reaches Rust as a TOML
 * array, parsed with `Comments::Literal`, so a leading `#` there is the first
 * character of a path.
 */
function syncVirtualPositivePatterns(raw: string): string[] {
  return splitSyncList(raw).filter((entry) => {
    const escaped = entry.startsWith("\\!") || entry.startsWith("\\#");
    if (!escaped && entry.startsWith("!")) {
      return false;
    }
    const body = escaped ? entry.slice(1) : entry;
    return body.replace(/^\//, "").replace(/\/$/, "") !== "";
  });
}

/**
 * The byte count the save sends for the size floor, and therefore the only
 * honest input to the sentence under the box (story 56.16).
 *
 * The note branched on `pinnedValue(virtualOverMb) === null`, which is true of a
 * blank box and false of `0.0000001`. That value rounds to zero bytes on the
 * wire, so Rust's `floor_selects` is false and nothing stays away at all, while
 * the form claimed "this size decides on its own" — the helper's own guarantee
 * inverted by a number the box's `step="any"` invites. It is the silent
 * coercion story 56.14's none-note closed, one order of magnitude further down:
 * `pinnedValue` stops at "not positive", and the rounding into Rust's `u64` is a
 * second floor underneath it that nothing on screen accounted for.
 *
 * The fix is not to send something else — a sub-byte floor genuinely is no floor
 * — but to make the save and the note read one function rather than one field.
 */
function syncVirtualOverBytes(virtualOverMb: string): number {
  const pinned = pinnedValue(virtualOverMb);
  return pinned === null ? 0 : Math.round(pinned * 1024 * 1024);
}

/** The LFS threshold the save sends, in bytes — keeper's own when nothing is pinned. */
function syncLfsThresholdBytes(lfsThresholdMb: string): number {
  const pinned = pinnedValue(lfsThresholdMb);
  return pinned === null ? SYNC_DEFAULT_LFS_THRESHOLD_BYTES : Math.round(pinned * 1024 * 1024);
}

/**
 * Which of the four floor sentences is true for what the two boxes hold now.
 *
 * Reuses {@link syncVirtualOverBytes} and {@link syncVirtualPositivePatterns} —
 * the byte count the save puts on the wire and Rust's own reading of the list —
 * and that is the point rather than thrift: the sentence on screen and the
 * policy the engine compiles are then decided by one reading of the form, so
 * they cannot drift. Their drifting IS the defect this replaces, twice over. The
 * old note described a pattern match while the save sent a floor with no pattern
 * beside it; the first repair then described a match whenever the box held any
 * entry at all, including a `!` line that authorizes nothing.
 *
 * The order matters. A positive entry is the only thing that takes the decision
 * away from the floor, so it is tested first; what is left is a box that is
 * empty and one that holds only protections, which differ in nothing but
 * whether there are exceptions to mention.
 *
 * Pure and exported so the four states can be asserted directly as well as
 * through the form: four mutually exclusive sentences are exactly the shape that
 * renders two of itself.
 */
export function syncVirtualOverNote(virtualOverMb: string, virtualPatterns: string): string {
  if (syncVirtualOverBytes(virtualOverMb) === 0) {
    return SYNC_VIRTUAL_OVER_NONE_NOTE;
  }
  if (syncVirtualPositivePatterns(virtualPatterns).length > 0) {
    return SYNC_VIRTUAL_OVER_MATCHED_NOTE;
  }
  return splitSyncList(virtualPatterns).length === 0
    ? SYNC_VIRTUAL_OVER_ALONE_NOTE
    : SYNC_VIRTUAL_OVER_PROTECTED_ONLY_NOTE;
}

/**
 * How the floor sits against the LFS threshold, as the line that says so — or
 * `null` when neither ordering has anything to report (story 56.16).
 *
 * ONE function returning WHICH line, not a boolean per line. The two orderings
 * are a partition of the same comparison, so a pair of independent predicates
 * could render both sentences at once or neither, and one of those two failures
 * is precisely what shipped: only `thresholdBytes < floorBytes` had a line, the
 * ordering in which the floor's own sentence is already true, and the inverse —
 * the one where the floor promises more than LFS will ever hand it — had none.
 *
 * Takes the whole form for `effectiveSettleSeconds`'s reason: the answer depends
 * on three controls at once, and reading them one at a time at the call site is
 * how a condition and the sentence it gates come apart. Compared in the bytes
 * both boxes are saved as, so the comparison cannot disagree with the wire about
 * which of the two is larger.
 */
function syncVirtualOverBandNote(form: SyncFormValues): string | null {
  const floorBytes = syncVirtualOverBytes(form.virtualOverMb);
  if (form.lfsMode !== "materialize" || floorBytes === 0) {
    return null;
  }
  return syncLfsThresholdBytes(form.lfsThresholdMb) < floorBytes
    ? SYNC_VIRTUAL_OVER_ABOVE_LFS_NOTE
    : SYNC_VIRTUAL_OVER_BELOW_LFS_NOTE;
}

/**
 * What the keychain answered when this form opened. It is the baseline every
 * save is judged against, and it is exactly the state the field cannot carry:
 * an empty box is what a removed token, a folder that never had one, and a read
 * that failed all look like.
 */
type StoredToken =
  | { readonly kind: "reading" }
  | { readonly kind: "stored"; readonly value: string }
  | { readonly kind: "absent" }
  | { readonly kind: "unreadable" };

/** The one keychain call a save makes for the token, or none at all. */
type CredentialWrite =
  | { readonly kind: "none" }
  | { readonly kind: "set"; readonly value: string }
  | { readonly kind: "clear" };

/**
 * Decide what a save does to the stored token, from the answer the form got
 * when it opened and what the field holds now (Story 34.12).
 *
 * The field cannot say this on its own. Once it opens pre-filled, an empty box
 * means "remove it" — and that is the same empty box produced by a folder with
 * no token and by a keychain that refused to answer. So only a `stored`
 * baseline licenses a removal: a locked keychain must not be able to destroy a
 * working credential by looking like a user who cleared the field. The price is
 * that a token cannot be removed while it cannot be read, which is the right
 * way round — the read failure is on screen and can be retried, a silent
 * deletion could not be.
 */
function credentialWrite(stored: StoredToken, field: string): CredentialWrite {
  if (stored.kind === "stored") {
    // Byte-identical to what was read back is the untouched field. Re-storing
    // the same secret is still a keychain write — and on some platforms a
    // prompt — for no change at all.
    if (field === stored.value) {
      return { kind: "none" };
    }
    return field === "" ? { kind: "clear" } : { kind: "set", value: field };
  }
  // Under the other three an empty field means nothing to do, for two different
  // reasons: `absent` because there is nothing to remove, `reading` and
  // `unreadable` because the form never learned whether there is. A typed value
  // is unambiguous under all three and still goes through.
  return field === "" ? { kind: "none" } : { kind: "set", value: field };
}

/**
 * The line under the field on an edit form. It is the whole report of what the
 * keychain answered, because two of the answers leave the field looking the
 * same while the difference between them is what saving will do.
 */
const TOKEN_NOTES: Record<StoredToken["kind"], string> = {
  reading: SYNC_TOKEN_READING_NOTE,
  stored: SYNC_TOKEN_EDIT_NOTE,
  absent: SYNC_TOKEN_NONE_STORED_NOTE,
  unreadable: SYNC_TOKEN_UNREADABLE_NOTE,
};

/**
 * Create a folder, or edit one that exists. A rejected save keeps every typed
 * value and shows the Rust validation message inline — losing a half-typed
 * remote URL to a validation error would be infuriating.
 *
 * @param profile - The profile to edit. Absent, the form creates a folder and
 *   behaves exactly as it always has. Present, it starts populated from this
 *   profile and submits with its id, which is what Rust reads as "update that
 *   one". Read once, on mount: the mirror behind it re-polls every couple of
 *   seconds, and re-seeding the fields from each snapshot would wipe out
 *   whatever was being typed when one landed.
 * @param disabled - Suppress every control while the caller's mirror is still
 *   unknown, so a folder cannot be added against a list nobody has read yet.
 * @param onSaved - Called after a profile is created or updated, with `settled`
 *   false while the form is still showing something only it can show: a
 *   keychain failure, whose message and — on an add — whose still-typed token
 *   live nowhere else. (It once also covered the stored-token acknowledgement
 *   and its Clear button; Story 34.12 deleted both, so a keychain failure is
 *   now the only unsettled ending.) A surface that hides the form on success
 *   must keep it mounted until `settled`, or it destroys the one place that
 *   failure is readable and the one field a retry can be driven from.
 * @param onCancel - Rendered as a Cancel button beside the submit. A surface
 *   that reveals the form behind an action passes this, so leaving changes
 *   nothing; one that keeps the form permanently on screen passes nothing.
 */
export function AddFolderForm({
  profile,
  disabled = false,
  className,
  onSaved,
  onCancel,
}: {
  profile?: SyncProfileVm;
  disabled?: boolean;
  className?: string;
  onSaved?: (profile: SyncProfileVm, settled: boolean) => void;
  onCancel?: () => void;
}) {
  const editing = profile !== undefined;
  // Seeded once, deliberately (see `profile` above).
  const [form, setForm] = useState<SyncFormValues>(() =>
    profile === undefined ? EMPTY_FORM : formValuesFor(profile),
  );
  const [expanded, setExpanded] = useState(false);
  /**
   * The profile keys this folder's own config file decides (Story 56.12).
   *
   * Read off the seeding profile rather than held in `form`, because it is not
   * something the form can change: it is a fact about a file on disk. Empty for
   * an add form, which has no folder bound yet, and for every folder with no
   * `.keeper/keeper.toml` — the normal case, in which nothing below this line
   * renders differently.
   */
  const folderOwned = new Set(profile?.folderOwned ?? []);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /**
   * What the keychain holds for this profile, as of when the form opened. A
   * folder being added has nothing stored under an id it does not have yet, so
   * that is `absent` and needs no read.
   */
  const [stored, setStored] = useState<StoredToken>(() =>
    profile === undefined ? { kind: "absent" } : { kind: "reading" },
  );
  /** What the keychain said when the read failed, for the line that reports it. */
  const [readError, setReadError] = useState<string | null>(null);
  /**
   * The folder this add form already created, when its token did not follow it
   * in (Story 34.12, finding 2 of the epic-34 review).
   *
   * An add whose profile write succeeded and whose keychain write failed leaves
   * the form standing with the typed token still in it, because that token is
   * the thing a retry has to deliver. But the folder now exists, so a retry
   * must *finish* it rather than create a second folder pointing at the same
   * path — which is precisely what a second `id: null` save would do. Holding
   * the created id here turns every subsequent Save on this form into an update
   * of that folder, and it is dropped again the moment a save completes whole.
   */
  const [createdId, setCreatedId] = useState<string | null>(null);
  /**
   * Whether the field shows what it holds. Mount-scoped on purpose: every
   * surface unmounts this form when it closes, so a reveal cannot outlive the
   * open it happened in and the next open starts masked.
   */
  const [tokenVisible, setTokenVisible] = useState(false);
  /**
   * This machine's home directory, or `null` while it is unknown (Story 59.8).
   *
   * Read from the shell rather than composed here, and read once per open: it
   * is what the Home control fills in, what a typed `~` resolves against, and
   * what the warning below compares the chosen folder to — so all three say the
   * same thing about the same machine or none of them do.
   */
  const [home, setHome] = useState<string | null>(null);
  const fieldId = useId();
  // Several folders can have an edit form open at once, so the name carries the
  // one it belongs to. A folder being added has no name of its own yet.
  const title = profile === undefined ? SYNC_ADD_TITLE : `${SYNC_EDIT_TITLE}: ${profile.name}`;
  // Read once: the JSX below both tests it and prints it, and calling the
  // helper twice is how a gate and the sentence it gates come apart.
  const virtualOverBandNote = syncVirtualOverBandNote(form);

  /**
   * Read the stored token into the field as the edit form opens (Story 34.12).
   *
   * This is the override of AD-34-7: the secret crosses the IPC boundary on a
   * form open rather than on a press. Keyed on the id and not the profile
   * object, because the mirror store hands this component a fresh object on
   * every refresh and re-running the read would overwrite what was typed since.
   */
  const profileId = profile?.id;
  useEffect(() => {
    if (profileId === undefined) {
      return;
    }
    let abandoned = false;
    void (async () => {
      try {
        const value = await syncGetCredential(profileId);
        if (abandoned) {
          return;
        }
        if (value === null) {
          setStored({ kind: "absent" });
          return;
        }
        setStored({ kind: "stored", value });
        // Only into a field nobody has started filling in: a keychain read can
        // be slower than typing, and an answer that overwrites what the user
        // just entered is worse than no answer.
        setForm((live) => (live.token === "" ? { ...live, token: value } : live));
      } catch (raw) {
        if (abandoned) {
          return;
        }
        setStored({ kind: "unreadable" });
        setReadError(syncErrorMessage(raw));
      }
    })();
    return () => {
      abandoned = true;
    };
  }, [profileId]);

  /**
   * What the notes flag currently IS on disk, as against what the form shows.
   *
   * A save writes the flag only when these two disagree, so a folder that is not
   * a vault — and was not one when the form opened — never touches the notes
   * subsystem on its way through Save.
   */
  const [storedNotesVault, setStoredNotesVault] = useState(false);
  const [storedNotesSubfolder, setStoredNotesSubfolder] = useState(SYNC_NOTES_DEFAULT_SUBFOLDER);

  /**
   * Seed the notes controls from the vault mirror as an edit form opens
   * (Story 37.1, FR-94).
   *
   * Whether a folder is a vault is not on `SyncProfileVm` — a vault is a
   * notes-flagged profile, and `notes_vaults` is the read that projects that.
   * So the switch starts off and is corrected once the mirror answers, into
   * fields the user has not touched, on the same "an answer must not overwrite
   * what was just typed" rule the keychain read above follows.
   */
  useEffect(() => {
    if (profileId === undefined) {
      return;
    }
    let abandoned = false;
    void (async () => {
      await ensureNotesVaultsHydrated();
      if (abandoned) {
        return;
      }
      const vault = notesVaultsStore
        .getState()
        .vaults?.find((candidate) => candidate.profileId === profileId);
      if (vault === undefined) {
        return;
      }
      setStoredNotesVault(true);
      setStoredNotesSubfolder(vault.subfolder);
      setForm((live) =>
        live.notesVault || live.notesSubfolder !== SYNC_NOTES_DEFAULT_SUBFOLDER
          ? live
          : { ...live, notesVault: true, notesSubfolder: vault.subfolder },
      );
    })();
    return () => {
      abandoned = true;
    };
  }, [profileId]);

  /**
   * Ask the shell where home is, once per open (Story 59.8).
   *
   * `homeDir()` is the path plugin's own answer — `dirs::home_dir()` in Rust,
   * which is `$HOME` on Linux and the `NSHomeDirectory` container on macOS —
   * so the one platform-specific rule in this story is answered by the layer
   * that knows the platform, not mirrored into TypeScript. Its permission is
   * already granted: `core:default` in `capabilities/default.json` carries
   * `core:path:default`, so this needs no new grant and no new command.
   *
   * A failure is swallowed and leaves `home` at `null`, which switches the
   * Home control off, stops `~` expanding and hides the warning. That is the
   * whole handling: nothing here is load-bearing enough to interrupt somebody
   * filling in a form with, and every path they can still type is checked by
   * Rust exactly as before. It also keeps this component mountable in the
   * suites that do not stub the shell at all, which is most of them.
   */
  useEffect(() => {
    let abandoned = false;
    void (async () => {
      try {
        const resolved = await homeDir();
        if (!abandoned) {
          // A trailing separator would make the warning's `=== home` comparison
          // fail against the very path the Home control just wrote.
          setHome(resolved.replace(/\/+$/, "") || "/");
        }
      } catch {
        // No home known; see above.
      }
    })();
    return () => {
      abandoned = true;
    };
  }, []);

  /**
   * The folder this form will actually send, and whether it IS home.
   *
   * Computed once for the same reason `virtualOverBandNote` is: the box, the
   * line under it, the warning and the save all have to be talking about one
   * path, and four calls to the same helper are four places for them to come
   * apart.
   */
  const resolvedPath = syncExpandHome(form.localPath, home);
  const homeChosen = home !== null && resolvedPath === home;

  /** Open the OS-native directory picker; a cancellation writes nothing. */
  const pickFolder = async () => {
    try {
      const selection = await openFolder({ directory: true });
      if (typeof selection === "string") {
        // Merge into the *live* values, not the render snapshot closed over
        // before the picker opened, so a field typed meanwhile survives.
        setForm((live) => ({ ...live, localPath: selection }));
      }
    } catch {
      // Picker cancellation / failure → keep the current folder (no write).
    }
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSaving(true);
    // Only the save's own error is reset: a failed keychain read still
    // describes the keychain, and it still governs what an empty field means.
    setError(null);
    // An empty box means "keeper picks", and keeper's own number is what it
    // means: `null` on the wire is the OMISSION Rust reads as "leave whatever is
    // stored" (AD-34-9), which is the opposite instruction. Sending the
    // documented default is not a hard-coded user opinion either — a stored
    // value equal to the default is exactly how `effective_settle_ms` encodes
    // "nothing pinned here", so a removable folder still gets its longer window.
    const settle = pinnedValue(form.settleSeconds);
    const poll = pinnedValue(form.pollSeconds);
    // The three virtualization knobs. Each is sent as the omission when this
    // folder's own config file owns the key: `profile::as_stored` would strip
    // the value out of the row anyway and log a warning nobody sees, so sending
    // it would only be this form claiming to have expressed something it did
    // not. Everywhere else all three are always expressed, because all three
    // are always on screen once Advanced is open — and their disclosure state
    // does not change that, the fields hold their seeded values either way.
    const virtualPatterns = folderOwned.has("virtualPatterns")
      ? null
      : splitSyncList(form.virtualPatterns);
    const virtualOverBytes = folderOwned.has("virtualOverBytes")
      ? null
      : syncVirtualOverBytes(form.virtualOverMb);
    const releaseTtlMs = folderOwned.has("releaseTtlMs")
      ? null
      : releaseTtlMsFor(form.releaseHours);
    const author = form.authorOverride.trim();
    // Decided before anything is written, against the answer this form opened
    // with, so a keychain read landing mid-save cannot change what the save
    // means. The field on its own cannot tell "remove it" from "keeper never
    // found out what is there".
    const credential = credentialWrite(stored, form.token);
    // Read off the form before anything is written, so the request cannot be
    // changed by whatever the awaits below do to the fields: the add branch
    // blanks the draft, but only once the whole save — profile *and* token —
    // has landed.
    const notesVault = form.notesVault;
    const notesSubfolder = form.notesSubfolder.trim();
    const recordings = form.recordings;
    const recordingsSubfolder = form.recordingsSubfolder.trim();
    const sessions = form.sessions;
    const sessionsSubfolder = form.sessionsSubfolder.trim();
    try {
      const saved = await saveSyncProfile({
        // Present updates that profile, absent creates one — the only field
        // that separates the two modes on the wire. The request carries no
        // `enabled`, and Rust merges an update onto the stored profile, so
        // saving an edit to a paused folder leaves it paused. `createdId`
        // stands in for it after an add that got its folder stored but not its
        // token, so the retry finishes that folder instead of adding another.
        id: profile?.id ?? createdId,
        name: form.name.trim(),
        // The path with `~` already resolved (Story 59.8), so what the line
        // under the box says is exactly what gets stored. On an edit this is a
        // no-op by construction — a stored path cleared `validate`'s absolute
        // check, so it cannot begin with a tilde — and the value is carried
        // back unchanged: the engine binds a profile to this path (and, on
        // removable media, to a marker under it), which is why the field above
        // is read-only rather than a second picker.
        localPath: resolvedPath,
        remoteUrl: form.remoteUrl.trim(),
        branch: form.branch.trim(),
        direction: form.direction,
        // The other lane (`worktree`) is an agent airlock Rust accepts only
        // together with `pushOnly`, so it stays a syncd-config affordance
        // rather than a control that can be combined into a rejected profile.
        lane: SYNC_DEFAULT_LANE,
        subpaths: splitSyncList(form.subpaths),
        excludes: splitSyncList(form.excludes),
        removable: form.removable,
        lfsMode: form.lfsMode,
        // Every key below that a folder config file can own, and that has an
        // `Option` slot, is sent as the omission when it is owned. `branch`,
        // `excludes` and `tags` can be owned too and have no omission to send —
        // their slots are bare — so those controls are disabled and re-send the
        // value in force. That is not free and the earlier claim that it was is
        // wrong: `profile::as_stored` compares the incoming value against the
        // TABLE row, and for an owned key those differ by definition, so every
        // such save logs a shadowed-change warning naming keys nobody could
        // touch. The data is safe — the value is restored — but the diagnostic
        // is noisy until those three slots become `Option` too, which is a wire
        // change and is recorded as deferred work.
        lfsThresholdBytes: folderOwned.has("lfsThresholdBytes")
          ? null
          : syncLfsThresholdBytes(form.lfsThresholdMb),
        virtualPatterns,
        virtualOverBytes,
        releaseTtlMs,
        settleMs: settle === null ? SYNC_DEFAULT_SETTLE_MS : Math.round(settle * 1000),
        pollIntervalMs: poll === null ? SYNC_DEFAULT_POLL_INTERVAL_MS : Math.round(poll * 1000),
        tags: splitSyncList(form.tags),
        // Emptying the field on an existing profile *clears* the override, and
        // only an explicit empty string says so — `null` is the omission Rust
        // reads as "leave whatever is stored". On a new profile there is
        // nothing to clear, so the omission is the more precise thing to send.
        authorOverride: author === "" && !editing ? null : author,
        // Always expressed, because the field is always on screen. An empty
        // string is a value here rather than an omission: it IS keeper's own
        // mechanical subject.
        commitSubjectTemplate: folderOwned.has("commitSubjectTemplate")
          ? null
          : form.commitSubjectTemplate.trim(),
        // The vault flag rides the profile save rather than a second command:
        // `notes` IS a field on the profile, so writing it here keeps the folder
        // and its vault-ness atomic and saves a round trip. Expressed whenever
        // the folder's own config file does not own it — `null` otherwise means
        // "this form does not show it", which is exactly true of a switch the
        // file has taken out of the person's hands (AD-34-9, AD-132). The
        // subfolder rides the same gate: it is the other half of one key.
        notes: folderOwned.has("notes") ? null : notesVault,
        // Only when the switch is on: the subfolder box is revealed with it, and
        // an unflagged save must not reset a subfolder the user chose earlier.
        notesSubfolder: folderOwned.has("notes") || !notesVault ? null : notesSubfolder,
        // The recordings flag, on the same terms as the vault flag above:
        // `recordings` IS a field on the profile, so it rides this save and the
        // folder and its recordings-ness land together. `false` REMOVES the
        // block rather than emptying it, which is what takes the folder back out
        // of the Recording destination picker.
        recordings: folderOwned.has("recordings") ? null : recordings,
        // Unflagged: say nothing, so an unflagged save cannot reset a subfolder
        // the owner chose earlier. Flagged and empty on an ADD: also nothing, and
        // keeper's own default stands — this form has no copy of it to send.
        // Flagged and empty on an EDIT: the box arrived holding the value in
        // force, so an empty one is a deliberate clear, and it goes as an empty
        // string for `RecordingsConfig::validate` to refuse in its own words.
        // Correcting it here to make the save succeed would be this form picking
        // a folder the owner did not name.
        recordingsSubfolder:
          folderOwned.has("recordings") || !recordings || (recordingsSubfolder === "" && !editing)
            ? null
            : recordingsSubfolder,
        // The sessions flag, on the recordings block's exact terms (AD-107):
        // `false` REMOVES the block, and the subfolder follows the recordings
        // empty-box rules.
        sessions: folderOwned.has("sessions") ? null : sessions,
        sessionsSubfolder:
          folderOwned.has("sessions") || !sessions || (sessionsSubfolder === "" && !editing)
            ? null
            : sessionsSubfolder,
      });
      // `saveSyncProfile` re-reads the profile/status mirror, but the Sync
      // view's three per-folder lists are a *second* mirror on a deliberately
      // slower poll. Reading them here — from whichever surface saved the
      // folder — is what keeps its card from sitting stale for a poll
      // interval. Never throws.
      await refreshSyncDetail(saved.id);
      // The switcher, the sidebar entry and the Notes pane all read the vault
      // mirror, and only a folder whose vault-ness actually moved can change it.
      // A folder that is not a vault, and was not one a moment ago, must not
      // reach into the notes subsystem at all.
      if (
        notesVault !== storedNotesVault ||
        (notesVault && notesSubfolder !== storedNotesSubfolder)
      ) {
        setStoredNotesVault(notesVault);
        setStoredNotesSubfolder(notesSubfolder);
        await refreshNoteVaults();
      }
      // The recordings flag needs no equivalent. There is no recordings mirror:
      // `recording_destination_profiles` is read by the destination card when it
      // mounts, so a folder flagged here is offered the next time the Recording
      // pane is opened, with nothing to keep in step in between.

      // The keychain leg, when the decision above says there is one. A second
      // write, to a different store, keyed by the profile id. Its failure is
      // reported as its own thing: the profile is stored by now, and a blanket
      // failure would be a lie.
      if (credential.kind !== "none") {
        try {
          if (credential.kind === "clear") {
            await syncClearCredential(saved.id);
          } else {
            await syncSetCredential(saved.id, credential.value);
          }
        } catch (raw) {
          // A removal is only reachable from an edit form, since an add has no
          // stored token to remove — but "not stored" would describe the
          // opposite of what was attempted, so it gets its own wording.
          let prefix = SYNC_TOKEN_FAILED_PREFIX;
          if (credential.kind === "clear") {
            prefix = SYNC_TOKEN_REMOVE_FAILED_PREFIX;
          } else if (editing) {
            prefix = SYNC_TOKEN_EDIT_FAILED_PREFIX;
          }
          setError(`${prefix}${syncErrorMessage(raw)}`);
          // The folder exists and its token does not, so a retry has to finish
          // *that* folder. Remembering its id is what stops the next Save from
          // creating a second one — see `createdId`.
          if (!editing) {
            setCreatedId(saved.id);
          }
          // The profile is stored and the keychain is not, so the caller is
          // told to keep the form up: the field still holds the value that has
          // to get in, and hiding the form would take it away. That is only
          // true because the add branch's reset is *below* this point — it ran
          // above it once, which silently destroyed the typed token on every
          // failed add and sent the user back to their forge for another PAT.
          onSaved?.(saved, false);
          return;
        }
        if (editing) {
          // The keychain now holds what the field holds, so the baseline moves
          // with it. Otherwise a second Save of an untouched form would rewrite
          // the same secret, and a save after a removal would try to remove it
          // again. An add form is about to be reset to a blank draft for the
          // next folder, whose baseline is still "nothing stored".
          setStored(
            credential.kind === "clear"
              ? { kind: "absent" }
              : { kind: "stored", value: credential.value },
          );
          setReadError(null);
          // The secret is committed; there is no reason to leave it legible.
          setTokenVisible(false);
        }
      }
      if (!editing) {
        // Both stores now hold what this form was for, so the draft is spent:
        // blank it for the next folder. Deliberately the last thing the add
        // path does, so that every earlier `return` leaves the typed values —
        // the token above all — where a retry can still reach them.
        setForm(EMPTY_FORM);
        setTokenVisible(false);
        setCreatedId(null);
      }
      onSaved?.(saved, true);
    } catch (raw) {
      setError(syncErrorMessage(raw));
    } finally {
      setSaving(false);
    }
  };

  const incomplete =
    form.name.trim() === "" || form.localPath === "" || form.remoteUrl.trim() === "";

  return (
    <form
      aria-label={title}
      className={cn("flex flex-col gap-2", className)}
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-name`}>{SYNC_NAME_LABEL}</Label>
        <Input
          id={`${fieldId}-name`}
          className="w-56"
          value={form.name}
          disabled={disabled || saving}
          onChange={(event) => setForm((live) => ({ ...live, name: event.target.value }))}
        />
      </div>
      {/* The folder, in two shapes rather than one (Story 59.8).

          EDITING is unchanged: a read-only line and no control at all, because
          the engine binds a profile to its folder. ADDING now offers a typed
          box beside the picker, which is what makes `~` reachable — the owner
          asked for a home-directory option and a picker alone cannot express
          one, since a native directory dialog has no way to say "the folder I
          mean is the one I can name". The box holds what was typed; the line
          under it says where that lands. */}
      {editing ? (
        <div className="flex items-center justify-between gap-2">
          <div className="flex min-w-0 flex-col gap-0.5">
            <Label>{SYNC_FOLDER_LABEL}</Label>
            <p
              className="truncate font-mono text-muted-foreground text-xs"
              data-testid={SYNC_FORM_PATH_TESTID}
              title={form.localPath === "" ? undefined : form.localPath}
            >
              {form.localPath === "" ? SYNC_NO_FOLDER_CHOSEN_LABEL : form.localPath}
            </p>
          </div>
        </div>
      ) : (
        <>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-path`}>{SYNC_FOLDER_LABEL}</Label>
            <Input
              id={`${fieldId}-path`}
              className="w-56 font-mono"
              placeholder={SYNC_FOLDER_PLACEHOLDER}
              value={form.localPath}
              disabled={disabled || saving}
              onChange={(event) => setForm((live) => ({ ...live, localPath: event.target.value }))}
            />
          </div>
          <div className="flex items-center justify-between gap-2">
            {/* The path as it will be stored, which is the same text as the box
                above unless a `~` was resolved out of it. Kept on screen even
                when it is a repetition: it is the only place a resolved tilde
                becomes a fact somebody can check before pressing Add, and the
                only line wide enough to read a long path in. */}
            <p
              className="min-w-0 truncate font-mono text-muted-foreground text-xs"
              data-testid={SYNC_FORM_PATH_TESTID}
              title={resolvedPath === "" ? undefined : resolvedPath}
            >
              {resolvedPath === "" ? SYNC_NO_FOLDER_CHOSEN_LABEL : resolvedPath}
            </p>
            <div className="flex shrink-0 items-center gap-2">
              {/* Fills the box with the resolved home directory rather than
                  with a `~`, so what is on screen from here on is the path that
                  will be stored. Off while the home directory is unknown: a
                  control that writes a guess is worse than one that is not
                  offered. */}
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={disabled || saving || home === null}
                onClick={() => {
                  if (home !== null) {
                    setForm((live) => ({ ...live, localPath: home }));
                  }
                }}
              >
                {SYNC_HOME_FOLDER_LABEL}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={disabled || saving}
                onClick={() => {
                  void pickFolder();
                }}
              >
                {SYNC_CHOOSE_FOLDER_LABEL}
              </Button>
            </div>
          </div>
        </>
      )}
      {/* Said in both modes, because it is a claim about what is being synced
          rather than about the press: somebody opening the form of a folder
          that already IS home needs it at least as much as somebody about to
          choose one. Not muted, unlike every other note in this form — the
          whole point of the sentence is that it is read once — and not
          `text-destructive` either, which in this file means a refusal, and
          this is not one: home is a legal folder and the save will go through.
          */}
      {homeChosen && <p className="text-foreground text-xs">{SYNC_HOME_FOLDER_WARNING}</p>}
      {editing && <p className="text-muted-foreground text-xs">{SYNC_PATH_FIXED_NOTE}</p>}
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-remote`}>{SYNC_REMOTE_URL_LABEL}</Label>
        <Input
          id={`${fieldId}-remote`}
          className="w-56"
          value={form.remoteUrl}
          disabled={disabled || saving}
          onChange={(event) => setForm((live) => ({ ...live, remoteUrl: event.target.value }))}
        />
      </div>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-branch`}>{SYNC_BRANCH_LABEL}</Label>
        <Input
          id={`${fieldId}-branch`}
          className="w-32"
          value={form.branch}
          disabled={disabled || saving || folderOwned.has("branch")}
          onChange={(event) => setForm((live) => ({ ...live, branch: event.target.value }))}
        />
      </div>
      {folderOwned.has("branch") && (
        <p className="text-muted-foreground text-xs">{syncFolderOwnedNote("branch")}</p>
      )}
      <div className="flex items-center justify-between gap-2">
        <Label id={`${fieldId}-direction-label`}>{SYNC_DIRECTION_LABEL}</Label>
        <Select
          value={form.direction}
          disabled={disabled || saving}
          onValueChange={(value) =>
            setForm((live) => ({ ...live, direction: value as SyncDirection }))
          }
        >
          <SelectTrigger
            className="w-40"
            data-testid={SYNC_DIRECTION_SELECT_TESTID}
            aria-labelledby={`${fieldId}-direction-label`}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {Object.entries(DIRECTION_LABELS).map(([value, label]) => (
              <SelectItem key={value} value={value}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {/* The notes flag (FR-94, AD-54). Above Advanced rather than inside it:
          it is the whole of what makes a folder a vault, and burying the one
          decision the feature needs behind a disclosure would be the "vault
          setup" flow the design exists to delete. */}
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-notes`}>{SYNC_NOTES_LABEL}</Label>
        <Switch
          id={`${fieldId}-notes`}
          checked={form.notesVault}
          disabled={disabled || saving || folderOwned.has("notes")}
          onCheckedChange={(checked) => setForm((live) => ({ ...live, notesVault: checked }))}
        />
      </div>
      <p className="text-muted-foreground text-xs">{SYNC_NOTES_NOTE}</p>
      {folderOwned.has("notes") && (
        <p className="text-muted-foreground text-xs">{syncFolderOwnedNote("notes")}</p>
      )}
      {form.notesVault && (
        <>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-notes-subfolder`}>{SYNC_NOTES_SUBFOLDER_LABEL}</Label>
            <Input
              id={`${fieldId}-notes-subfolder`}
              className="w-56"
              // Prefilled with keeper's real default rather than left blank: the
              // common answer is already given, and the resolved path below can
              // then be a fact instead of a preview of a guess.
              placeholder={SYNC_NOTES_DEFAULT_SUBFOLDER}
              value={form.notesSubfolder}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, notesSubfolder: event.target.value }))
              }
            />
          </div>
          {form.localPath !== "" && (
            <p className="truncate font-mono text-muted-foreground text-xs">
              {`${form.localPath}/${form.notesSubfolder.trim() === "" ? SYNC_NOTES_DEFAULT_SUBFOLDER : form.notesSubfolder.trim()}`}
            </p>
          )}
          {/* Three lines, not a docs link: these are claims about the user's own
              files, and a claim you have to go and look up is a claim. */}
          <ul className="flex flex-col gap-0.5">
            {SYNC_NOTES_GUARANTEES.map((line) => (
              <li key={line} className="text-muted-foreground text-xs">
                {line}
              </li>
            ))}
          </ul>
        </>
      )}
      {/* The recordings flag (AD-66). Beside the notes flag, above Advanced, for
          the same reason: it is the whole of what makes a folder a recording
          destination, and it is the one control Stories 41.1 and 41.2 were
          waiting on. */}
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-recordings`}>{SYNC_RECORDINGS_LABEL}</Label>
        <Switch
          id={`${fieldId}-recordings`}
          checked={form.recordings}
          disabled={disabled || saving || folderOwned.has("recordings")}
          onCheckedChange={(checked) => setForm((live) => ({ ...live, recordings: checked }))}
        />
      </div>
      <p className="text-muted-foreground text-xs">{SYNC_RECORDINGS_NOTE}</p>
      {folderOwned.has("recordings") && (
        <p className="text-muted-foreground text-xs">{syncFolderOwnedNote("recordings")}</p>
      )}
      {form.recordings && (
        <>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-recordings-subfolder`}>
              {SYNC_RECORDINGS_SUBFOLDER_LABEL}
            </Label>
            <Input
              id={`${fieldId}-recordings-subfolder`}
              className="w-56"
              value={form.recordingsSubfolder}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, recordingsSubfolder: event.target.value }))
              }
            />
          </div>
          {/* Only while adding, where the box starts empty because no stored
              profile has told this form what keeper would pick. */}
          {!editing && (
            <p className="text-muted-foreground text-xs">{SYNC_RECORDINGS_SUBFOLDER_NOTE}</p>
          )}
          {/* The resolved root, and only ever of what the box actually holds. An
              emptied box on an edit form previews nothing rather than the stored
              value it no longer says — that save is about to be refused, and a
              preview of a path it will not write would be the form disagreeing
              with itself. */}
          {form.localPath !== "" && form.recordingsSubfolder.trim() !== "" && (
            <p className="truncate font-mono text-muted-foreground text-xs">
              {`${form.localPath}/${form.recordingsSubfolder.trim()}`}
            </p>
          )}
        </>
      )}
      {/* The sessions flag (FR-222, AD-107). Third in the "this folder also
          holds X" row, on the recordings control's exact shape. */}
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-sessions`}>{SYNC_SESSIONS_LABEL}</Label>
        <Switch
          id={`${fieldId}-sessions`}
          checked={form.sessions}
          disabled={disabled || saving || folderOwned.has("sessions")}
          onCheckedChange={(checked) => setForm((live) => ({ ...live, sessions: checked }))}
        />
      </div>
      <p className="text-muted-foreground text-xs">{SYNC_SESSIONS_NOTE}</p>
      {folderOwned.has("sessions") && (
        <p className="text-muted-foreground text-xs">{syncFolderOwnedNote("sessions")}</p>
      )}
      {form.sessions && (
        <>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-sessions-subfolder`}>{SYNC_SESSIONS_SUBFOLDER_LABEL}</Label>
            <Input
              id={`${fieldId}-sessions-subfolder`}
              className="w-56"
              value={form.sessionsSubfolder}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, sessionsSubfolder: event.target.value }))
              }
            />
          </div>
          {/* Only while adding, for the recordings reason: no stored profile has
              told this form what keeper would pick. */}
          {!editing && (
            <p className="text-muted-foreground text-xs">{SYNC_SESSIONS_SUBFOLDER_NOTE}</p>
          )}
          {form.localPath !== "" && form.sessionsSubfolder.trim() !== "" && (
            <p className="truncate font-mono text-muted-foreground text-xs">
              {`${form.localPath}/${form.sessionsSubfolder.trim()}`}
            </p>
          )}
        </>
      )}
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="w-fit justify-start gap-1 px-1"
        data-testid={SYNC_ADVANCED_TOGGLE_TESTID}
        aria-expanded={expanded}
        onClick={() => setExpanded((shown) => !shown)}
      >
        {expanded ? <ChevronDown aria-hidden /> : <ChevronRight aria-hidden />}
        {SYNC_ADVANCED_LABEL}
      </Button>
      {expanded && (
        <div className="flex flex-col gap-2 pl-1">
          <div className="flex items-center justify-between gap-2">
            <Label id={`${fieldId}-lfs-label`}>{SYNC_LFS_MODE_LABEL}</Label>
            <Select
              value={form.lfsMode}
              disabled={disabled || saving}
              onValueChange={(value) =>
                setForm((live) => ({ ...live, lfsMode: value as SyncLfsMode }))
              }
            >
              <SelectTrigger
                className="w-52"
                data-testid={SYNC_LFS_SELECT_TESTID}
                aria-labelledby={`${fieldId}-lfs-label`}
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.entries(LFS_MODE_LABELS).map(([value, label]) => (
                  <SelectItem key={value} value={value}>
                    {label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-threshold`}>{SYNC_LFS_THRESHOLD_LABEL}</Label>
            {/* MB is a scale the user did not choose, so a fraction is ordinary
                here: keeper's own documented 256 KiB threshold is 0.25 of one.
                Without a `step` HTML's implicit step is 1, which makes every
                fraction a stepMismatch — and because this box sits in a real
                form with a native submit and no `noValidate`, WKWebView refuses
                the whole save, including one that came to change something else.
                `step="any"` with `inputMode="decimal"` is the pair
                `session-space-editor.tsx:566-568` already uses for a fractional
                box; the integer box beside it there keeps `step="1"`, so the two
                cases stay distinguishable rather than uniformly loosened.
                Downstream needs nothing: `pinnedValue` parses with
                `Number.parseFloat`, and the one rounding at the save keeps the
                byte count integral for Rust's `u64`. */}
            <Input
              id={`${fieldId}-threshold`}
              type="number"
              min={0}
              step="any"
              inputMode="decimal"
              className="w-24"
              placeholder={String(SYNC_DEFAULT_LFS_THRESHOLD_BYTES / 1024 / 1024)}
              value={form.lfsThresholdMb}
              disabled={disabled || saving || folderOwned.has("lfsThresholdBytes")}
              onChange={(event) =>
                setForm((live) => ({ ...live, lfsThresholdMb: event.target.value }))
              }
            />
          </div>
          {folderOwned.has("lfsThresholdBytes") && (
            <p className="text-muted-foreground text-xs">
              {syncFolderOwnedNote("lfsThresholdBytes")}
            </p>
          )}
          {/* The three virtual-file controls (Story 56.12). They sit here, next
              to the LFS pair and above everything else, because they are about
              the same files: a path can only leave its content behind if keeper
              routed that content through LFS to begin with. */}
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-virtual-patterns`}>{SYNC_VIRTUAL_PATTERNS_LABEL}</Label>
            <Input
              id={`${fieldId}-virtual-patterns`}
              className="w-56"
              value={form.virtualPatterns}
              disabled={disabled || saving || folderOwned.has("virtualPatterns")}
              onChange={(event) =>
                setForm((live) => ({ ...live, virtualPatterns: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_VIRTUAL_PATTERNS_NOTE}</p>
          {folderOwned.has("virtualPatterns") && (
            <p className="text-muted-foreground text-xs">
              {syncFolderOwnedNote("virtualPatterns")}
            </p>
          )}
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-virtual-over`}>{SYNC_VIRTUAL_OVER_LABEL}</Label>
            {/* `step="any"` for the threshold box's reason directly above: a
                fraction of a megabyte is an ordinary answer here, and the
                implicit step of 1 would turn one into a stepMismatch that makes
                WKWebView refuse the whole submit. */}
            <Input
              id={`${fieldId}-virtual-over`}
              type="number"
              min={0}
              step="any"
              inputMode="decimal"
              className="w-24"
              placeholder="0"
              value={form.virtualOverMb}
              disabled={disabled || saving || folderOwned.has("virtualOverBytes")}
              onChange={(event) =>
                setForm((live) => ({ ...live, virtualOverMb: event.target.value }))
              }
            />
          </div>
          {/* Exactly one floor sentence, chosen by {@link syncVirtualOverNote}
              from the same readings of the two boxes the save runs — so what
              this claims and what the wire carries cannot come apart. It was
              two <p>s: an unconditional line about matched files plus the
              no-floor line, so a blank box showed both and the pair
              contradicted itself, while the owner's floor-with-no-patterns
              state had no true sentence at all. Rendered even when the folder's
              file owns the key, because the value is still in force and a
              person still has to be told what it does. */}
          <p className="text-muted-foreground text-xs">
            {syncVirtualOverNote(form.virtualOverMb, form.virtualPatterns)}
          </p>
          {/* And at most one band line. The two orderings come out of a single
              comparison for the same reason the floor sentence does. */}
          {virtualOverBandNote !== null && (
            <p className="text-muted-foreground text-xs">{virtualOverBandNote}</p>
          )}
          {folderOwned.has("virtualOverBytes") && (
            <p className="text-muted-foreground text-xs">
              {syncFolderOwnedNote("virtualOverBytes")}
            </p>
          )}
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-release-ttl`}>{SYNC_RELEASE_TTL_LABEL}</Label>
            <Input
              id={`${fieldId}-release-ttl`}
              type="number"
              min={0}
              step="any"
              inputMode="decimal"
              className="w-24"
              placeholder={String(SYNC_DEFAULT_RELEASE_TTL_MS / MS_PER_HOUR)}
              value={form.releaseHours}
              disabled={disabled || saving || folderOwned.has("releaseTtlMs")}
              onChange={(event) =>
                setForm((live) => ({ ...live, releaseHours: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_RELEASE_TTL_NOTE}</p>
          {/* `0` is not "keeper picks" here, it is an instruction — so it gets
              said back in words rather than left to the reader to infer from a
              box holding a zero. */}
          {releaseTtlMsFor(form.releaseHours) === 0 && (
            <p className="text-muted-foreground text-xs">{SYNC_RELEASE_NEVER_NOTE}</p>
          )}
          {/* A box holding something Rust will not use verbatim. Compared
              against the SUBSTITUTION rather than against the raw product:
              `1.1 * 3_600_000` is `3960000.0000000005` in binary floating point,
              so a comparison with the unrounded product fired this note on
              ordinary one-decimal input and then told the reader keeper was
              using the very number they had typed. Only the coercions
              `releaseTtlMsFor` actually performs reach here. */}
          {releaseTtlMsFor(form.releaseHours) !== 0 &&
            !Number.isNaN(Number.parseFloat(form.releaseHours)) &&
            releaseTtlMsFor(form.releaseHours) !==
              Math.round(Number.parseFloat(form.releaseHours) * MS_PER_HOUR) && (
              <p className="text-muted-foreground text-xs">
                {syncReleaseInForceNote(releaseTtlMsFor(form.releaseHours) / MS_PER_HOUR)}
              </p>
            )}
          {/* An empty box is the 24 h default rather than a value Rust would
              refuse, so it is not covered above — and neither is a window Rust
              refuses outright, which needs saying before the save rather than
              after it. */}
          {Number.isNaN(Number.parseFloat(form.releaseHours)) && (
            <p className="text-muted-foreground text-xs">
              {syncReleaseInForceNote(SYNC_DEFAULT_RELEASE_TTL_MS / MS_PER_HOUR)}
            </p>
          )}
          {releaseTtlMsFor(form.releaseHours) !== 0 &&
            (releaseTtlMsFor(form.releaseHours) < SYNC_MIN_RELEASE_TTL_MS ||
              releaseTtlMsFor(form.releaseHours) > SYNC_RELEASE_TTL_CEILING_MS) && (
              <p className="text-muted-foreground text-xs">{SYNC_RELEASE_OUT_OF_RANGE_NOTE}</p>
            )}
          {folderOwned.has("releaseTtlMs") && (
            <p className="text-muted-foreground text-xs">{syncFolderOwnedNote("releaseTtlMs")}</p>
          )}
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-removable`}>{SYNC_REMOVABLE_LABEL}</Label>
            <Checkbox
              id={`${fieldId}-removable`}
              checked={form.removable}
              disabled={disabled || saving}
              onCheckedChange={(checked) =>
                setForm((live) => ({ ...live, removable: checked === true }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_REMOVABLE_NOTE}</p>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-settle`}>{SYNC_SETTLE_LABEL}</Label>
            <Input
              id={`${fieldId}-settle`}
              type="number"
              min={0}
              // Fractional for the same reason as the threshold above: a 7.5 s
              // window is a legal wait, the save already rounds it to whole
              // milliseconds, and the implicit step=1 would block the submit.
              step="any"
              inputMode="decimal"
              className="w-24"
              // Empty means "keeper picks", so the box shows nothing and the
              // placeholder names what keeper will pick — 10 s on removable
              // storage, and it follows the checkbox above live.
              placeholder={String(effectiveSettleSeconds({ ...form, settleSeconds: "" }))}
              value={form.settleSeconds}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, settleSeconds: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_SETTLE_NOTE}</p>
          {/* A typed number Rust will not use verbatim. The reachable case is a
              wait of exactly keeper's own default on removable storage, which
              Rust reads as "nothing pinned" and answers with the longer window
              — so the box would otherwise show 5 while 10 was in force. */}
          {pinnedValue(form.settleSeconds) !== null &&
            pinnedValue(form.settleSeconds) !== effectiveSettleSeconds(form) && (
              <p className="text-muted-foreground text-xs">
                {syncInForceNote(effectiveSettleSeconds(form))}
              </p>
            )}
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-poll`}>{SYNC_POLL_LABEL}</Label>
            <Input
              id={`${fieldId}-poll`}
              type="number"
              min={0}
              // Fractional, as the wait above — the save rounds to whole ms.
              step="any"
              inputMode="decimal"
              className="w-24"
              placeholder={String(effectivePollSeconds({ ...form, pollSeconds: "" }))}
              value={form.pollSeconds}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, pollSeconds: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_POLL_NOTE}</p>
          {/* Here the divergence is the floor: a cadence under two seconds would
              put a full re-stat of the tree on every supervisor tick. */}
          {pinnedValue(form.pollSeconds) !== null &&
            pinnedValue(form.pollSeconds) !== effectivePollSeconds(form) && (
              <p className="text-muted-foreground text-xs">
                {syncInForceNote(effectivePollSeconds(form))}
              </p>
            )}
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-excludes`}>{SYNC_EXCLUDES_LABEL}</Label>
            <Input
              id={`${fieldId}-excludes`}
              className="w-56"
              value={form.excludes}
              disabled={disabled || saving || folderOwned.has("excludes")}
              onChange={(event) => setForm((live) => ({ ...live, excludes: event.target.value }))}
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_EXCLUDES_NOTE}</p>
          {folderOwned.has("excludes") && (
            <p className="text-muted-foreground text-xs">{syncFolderOwnedNote("excludes")}</p>
          )}
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-subpaths`}>{SYNC_SUBPATHS_LABEL}</Label>
            <Input
              id={`${fieldId}-subpaths`}
              className="w-56"
              value={form.subpaths}
              disabled={disabled || saving}
              onChange={(event) => setForm((live) => ({ ...live, subpaths: event.target.value }))}
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_SUBPATHS_NOTE}</p>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-tags`}>{SYNC_TAGS_LABEL}</Label>
            <Input
              id={`${fieldId}-tags`}
              className="w-56"
              value={form.tags}
              disabled={disabled || saving || folderOwned.has("tags")}
              onChange={(event) => setForm((live) => ({ ...live, tags: event.target.value }))}
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_TAGS_NOTE}</p>
          {folderOwned.has("tags") && (
            <p className="text-muted-foreground text-xs">{syncFolderOwnedNote("tags")}</p>
          )}
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-subject`}>{SYNC_SUBJECT_LABEL}</Label>
            <Input
              id={`${fieldId}-subject`}
              className="w-56"
              // The placeholder is what an empty field produces, so it is the
              // real subject rather than an example: `{profile}` is substituted
              // with the name being typed, and the counts stand for whatever a
              // commit turns out to carry.
              placeholder={`sync(${form.name.trim() === "" ? "folder" : form.name.trim()}): 3 added, 1 modified`}
              value={form.commitSubjectTemplate}
              disabled={disabled || saving || folderOwned.has("commitSubjectTemplate")}
              onChange={(event) =>
                setForm((live) => ({ ...live, commitSubjectTemplate: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_SUBJECT_NOTE}</p>
          {folderOwned.has("commitSubjectTemplate") && (
            <p className="text-muted-foreground text-xs">
              {syncFolderOwnedNote("commitSubjectTemplate")}
            </p>
          )}
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-author`}>{SYNC_AUTHOR_LABEL}</Label>
            <Input
              id={`${fieldId}-author`}
              className="w-56"
              value={form.authorOverride}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, authorOverride: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_AUTHOR_NOTE}</p>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-token`}>{SYNC_TOKEN_LABEL}</Label>
            <div className="flex w-56 items-center gap-1">
              <Input
                id={`${fieldId}-token`}
                type={tokenVisible ? "text" : "password"}
                autoComplete="off"
                className="min-w-0 flex-1"
                value={form.token}
                disabled={disabled || saving}
                onChange={(event) => {
                  setForm((live) => ({ ...live, token: event.target.value }));
                }}
              />
              {/* A button, not an adornment with a click handler: it changes
                  what is on screen, so it has to be reachable by keyboard, carry
                  its state, and say which way it will flip. */}
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-pressed={tokenVisible}
                aria-label={tokenVisible ? SYNC_TOKEN_HIDE_LABEL : SYNC_TOKEN_SHOW_LABEL}
                disabled={disabled || saving}
                onClick={() => setTokenVisible((shown) => !shown)}
              >
                {tokenVisible ? <EyeOff /> : <Eye />}
              </Button>
            </div>
          </div>
          <p className="text-muted-foreground text-xs">
            {editing ? TOKEN_NOTES[stored.kind] : SYNC_TOKEN_NOTE}
          </p>
          {/* What the keychain said, kept beside the field it explains. The
              note above already says the field will not be acted on; this says
              why, in the keychain's own words, so the user can decide whether
              it is worth reopening the form. */}
          {readError !== null && (
            <p className="text-destructive text-xs">
              {SYNC_TOKEN_READ_FAILED_PREFIX}
              {readError}
            </p>
          )}
        </div>
      )}
      {error !== null && <p className="text-destructive text-xs">{error}</p>}
      <div className="flex items-center gap-2">
        <Button
          type="submit"
          variant="outline"
          size="sm"
          className="w-fit"
          disabled={disabled || saving || incomplete}
        >
          {editing ? SYNC_EDIT_SUBMIT_LABEL : SYNC_ADD_SUBMIT_LABEL}
        </Button>
        {onCancel !== undefined && (
          <Button type="button" variant="ghost" size="sm" disabled={saving} onClick={onCancel}>
            {SYNC_FORM_CANCEL_LABEL}
          </Button>
        )}
      </div>
    </form>
  );
}
