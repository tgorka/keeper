/**
 * Which category of Settings each section sits in (Epic 102, Story 102.8).
 *
 * Settings was one long scroll of every section; it is now a short list of
 * categories, each showing only its own sections. This table is the whole of
 * that regrouping, and it is pure UI: a section is the same component reading
 * and writing the same keys wherever it sits, so nothing here can change what
 * syncs (the owner's "but still everything synced").
 *
 * Both surfaces read it — the pane's left list on a desktop and the drill-in
 * list on a phone and in the dialog — so a section cannot be in a category on
 * one surface and missing from it on the other. Every section id is in exactly
 * one category: `settings-categories.test.tsx` renders every category and
 * checks that nothing is lost and nothing shows twice.
 *
 * Order inside a category is the order its sections render. Account comes
 * first because an account is how settings arrive on a device (UX-DR116).
 */

/** Every Settings section, by the id deep links and the category table use. */
export const SETTINGS_SECTION_IDS = [
  "account",
  "device-name",
  "device",
  "encryption",
  "setup",
  "notifications",
  "background",
  "default-agent",
  "agents-server",
  "agent-instances",
  "always-allowed",
  "old-agent-rooms",
  "voice",
  "sessions",
  "capture",
  "notes-search",
  "model-providers",
  "sync",
  "tasks",
  "sync-git",
  "recording",
  "transcription",
  "shortcuts",
  "privacy",
  "storage",
  "diagnostics",
  "about",
  "config-sources",
] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTION_IDS)[number];

export type SettingsCategoryId =
  | "account"
  | "notifications"
  | "agents"
  | "notes"
  | "sync"
  | "recording"
  | "shortcuts"
  | "privacy"
  | "about"
  | "advanced";

export interface SettingsCategory {
  id: SettingsCategoryId;
  /** What the list shows and the category's heading says. */
  title: string;
  /** Its sections, in the order they render. */
  sections: readonly SettingsSectionId[];
}

export const SETTINGS_CATEGORIES: readonly SettingsCategory[] = [
  {
    id: "account",
    title: "Account & device",
    sections: ["account", "device-name", "device", "encryption", "setup"],
  },
  { id: "notifications", title: "Notifications & dock", sections: ["notifications", "background"] },
  {
    id: "agents",
    title: "Agents & voice",
    sections: [
      "default-agent",
      "agents-server",
      "agent-instances",
      "always-allowed",
      "old-agent-rooms",
      "voice",
      "sessions",
    ],
  },
  {
    id: "notes",
    title: "Notes",
    sections: ["capture", "notes-search", "model-providers"],
  },
  { id: "sync", title: "Sync & drives", sections: ["sync", "tasks", "sync-git"] },
  { id: "recording", title: "Recording & transcription", sections: ["recording", "transcription"] },
  { id: "shortcuts", title: "Shortcuts", sections: ["shortcuts"] },
  {
    id: "privacy",
    title: "Privacy & diagnostics",
    sections: ["privacy", "storage", "diagnostics"],
  },
  { id: "about", title: "Updates & about", sections: ["about"] },
  { id: "advanced", title: "Advanced", sections: ["config-sources"] },
];

/** The category `section` sits in. */
export function categoryOf(section: SettingsSectionId): SettingsCategoryId {
  const found = SETTINGS_CATEGORIES.find((category) => category.sections.includes(section));
  // Unreachable while every id is in the table, which the category test pins;
  // the account category is the honest place to land if it ever is not.
  return found?.id ?? "account";
}
