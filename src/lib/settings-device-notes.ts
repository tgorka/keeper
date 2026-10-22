/**
 * What of Settings is this device's own, said under the category where each
 * one sits (Story 102.8, claim 9).
 *
 * Two different things, kept apart because a person checking that every
 * setting lives in their account must be able to tell them apart:
 *
 * - **Applies only to this device — backed up in your account.** A settings
 *   key `settings_sync::synced_file_of` routes to `<login>/settings.<device>.toml`:
 *   this device's own answer, which leaves the device with the account and comes
 *   back when the device is set up again, but never becomes another device's.
 * - **Kept only on this device.** Not a settings key at all — a login item the
 *   operating system keeps, a secret in this device's keychain, the drive
 *   profile's capture template (DW-1580) — so there is nothing to back up.
 *
 * A note that names keys is the first kind, and that is all it says: where the
 * keys travel is the registry's answer, and `settings-device-notes.test.ts`
 * reads it from the generated `docs/settings-keys.md` and fails if any key a
 * note names is not in this device's own file. Only a note about something that
 * is not a key says where it is kept, because no registry can.
 */

/** Where a note's subject lives. */
export type DeviceNoteKind = "account" | "device";

export type DeviceNote =
  | {
      text: string;
      /** The settings keys it is about; the registry places them. */
      keys: readonly [string, ...string[]];
    }
  | {
      text: string;
      keys?: undefined;
      /** Not a key, so the registry cannot say: where it is kept. */
      kept: DeviceNoteKind;
    };

export function kindOf(note: DeviceNote): DeviceNoteKind {
  return note.keys === undefined ? note.kept : "account";
}

/** The heading over notes whose subject is backed up in the account. */
export const DEVICE_FILE_TITLE = "Applies only to this device — backed up in your account";

/** The heading over notes whose subject never leaves the device. */
export const DEVICE_ONLY_TITLE = "Kept only on this device";

export const SHORTCUTS_NOTE: DeviceNote = {
  text: "Shortcuts: a shortcut is registered with this machine's window server, and two machines cannot agree on one that is free on both.",
  keys: ["hotkey.global", "hotkey.recording", "hotkey.capture", "hotkey.voice"],
};

export const RECORDING_FOLDER_NOTE: DeviceNote = {
  text: "Where recordings are saved: it is a folder on this machine's disk, which is not there on another machine.",
  keys: ["recording.destination_dir", "recording.destination_profile_id"],
};

export const DRIVE_FOLDER_NOTE: DeviceNote = {
  text: "Where drives added from your repositories go: it is a folder on this machine's disk, which is not there on another machine.",
  keys: ["sync.drive_folder"],
};

export const SYNC_GIT_NOTE: DeviceNote = {
  text: "The git keeper uses: it is a path to a program on this machine, and git is somewhere else on another machine.",
  keys: ["sync.git_path"],
};

export const TASKS_LEDGER_NOTE: DeviceNote = {
  text: "Which drive holds task ledgers: it names this machine's copy of the drive, and the same folder has another id on another machine.",
  keys: ["tasks.ledger_vault"],
};

export const STORE_POSTURE_NOTE: DeviceNote = {
  text: "Whether the Matrix stores are encrypted at rest: it describes this device's own stores, and changing it re-keys them.",
  keys: ["sdk_encryption"],
};

export const STORE_PASSPHRASE_NOTE: DeviceNote = {
  text: "The passphrase this device's Matrix stores are encrypted with: it is in this machine's Keychain.",
  kept: "device",
};

export const VOICE_NOTE: DeviceNote = {
  text: "Listening for the wake phrase, and the voice language: an open microphone is a person's tap on the device that listens, and each device runs its own speech models. Setting this device up again brings the language back and leaves listening off.",
  keys: ["voice.wake_enabled", "voice.locale"],
};

export const ENDPOINT_KEY_NOTE: DeviceNote = {
  text: "A model provider's key: it is in this device's keychain. The provider itself is offered to your other devices.",
  kept: "device",
};

export const LAUNCH_AT_LOGIN_NOTE: DeviceNote = {
  text: "Launch at login: it is a login item macOS keeps for this Mac, not a keeper setting.",
  kept: "device",
};

export const CAPTURE_NOTE: DeviceNote = {
  text: "A drive's capture template and tag: keeper keeps them with this device's copy of the drive, so set them on each device.",
  kept: "device",
};

export const DEVICE_NAME_NOTE: DeviceNote = {
  text: "This device's name: it is kept for this device in your account, and published with every change it commits, so anyone who reads a drive's history sees it.",
  kept: "account",
};
