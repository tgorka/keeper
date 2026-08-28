/**
 * Settings → where each setting actually comes from (Story 46.7, AD-98).
 *
 * # Why this exists at all
 *
 * AD-98 replaced a boot-time import with a read-time layer stack, and that
 * change is only half a feature. `config.json` used to lose to the first UI
 * toggle after boot, which is why nobody used it. A layer keeps winning — which
 * means the *toggle* now loses, every time, forever. Without a surface, the
 * visible effect of the fix is a switch that flips back on its own: worse than
 * the bug it replaced, because the old one at least only happened once.
 *
 * So the requirement is precise and it is the whole of this file: **a control
 * that would be overridden says so instead of silently losing**
 * ({@link FileControlled}), and there is one place that lists every such key
 * with the file that decided it ({@link ConfigSourceSection}).
 *
 * # What this is not
 *
 * Not a TOML editor. The premise of the epic is that the file is the setting
 * and the owner edits the file; a surface that wrote the file back would
 * reintroduce the two-writers problem the layer stack exists to end. Every
 * path here is shown so it can be opened somewhere else.
 */
import { useEffect } from "react";

import { Badge } from "@/components/ui/badge";
import {
  CONFIG_LAYERS_UNREADABLE,
  refreshConfigLayers,
  useConfigLayersStore,
  useSettingOverride,
} from "@/lib/stores/config-layers";

/** Block heading. Named for the question it answers, not for the mechanism. */
export const CONFIG_SOURCE_TITLE = "Where your settings come from";

/**
 * The badge a file-decided control wears.
 *
 * Two words, because it sits inside a row that already has a label and a
 * control and must not push either of them out of shape. The sentence that
 * explains it rides on the badge itself, for the reader who stops on it.
 */
export const FILE_CONTROLLED_LABEL = "Set by a file";

/**
 * Introduces the list. The summary sentence above it carries the counts.
 *
 * Deliberately not the same words as {@link FILE_CONTROLLED_LABEL}: a heading
 * and a badge that read identically make "the marker on this control" and "the
 * heading of that list" indistinguishable to anything that looks the surface up
 * by its text — a screen reader, and a test.
 */
export const CONFIG_SOURCE_OVERRIDES_TITLE = "Decided by a file";

/** Introduces the faults. Named as a consequence, not as a category. */
export const CONFIG_SOURCE_FAULTS_TITLE = "These did not load";

/** Precedes the designated main sync folder, when one is designated. */
export const CONFIG_SOURCE_MAIN_FOLDER_LABEL = "Shared settings folder";

/**
 * The sentence a file-decided control carries, composed once so the section and
 * the badge cannot word the same fact two ways.
 *
 * Exported because it is what the badge's accessible name and its tooltip both
 * are, and what a test asserts.
 */
export function fileControlledDetail(key: string, source: string, path: string): string {
  return `${key} is set by ${source} (${path}). Changing it here will not take effect while that file sets it.`;
}

/**
 * The marker a control wears when a settings file decides its value.
 *
 * **It says so; it does not disable.** Disabling would be the tidier-looking
 * choice and it would be wrong: `set_setting` still writes the settings table,
 * the table is still the fallback, and the value a user sets here is exactly
 * what takes effect the moment the file stops setting the key. A disabled
 * control would make the honest fallback unreachable and turn a temporary
 * override into a permanent one.
 *
 * Renders nothing at all when no file decides `key`, which is the normal case
 * for every control in Settings on an install with no `keeper.toml`.
 */
export function FileControlled({ settingKey }: { settingKey: string }) {
  const override = useSettingOverride(settingKey);
  if (override === null) {
    return null;
  }
  const detail = fileControlledDetail(settingKey, override.source, override.path);
  return (
    <Badge variant="outline" role="note" aria-label={detail} title={detail}>
      {FILE_CONTROLLED_LABEL}
    </Badge>
  );
}

/**
 * The section that answers the question for every key at once.
 *
 * Rendered unconditionally, including on the install where nothing is
 * overridden — the `SyncGitRow` argument. A section that appeared only once a
 * file already existed would be invisible to everyone who has not yet
 * discovered that files are possible, and its one quiet line is the cheapest
 * way to say that settings can live in a file the owner edits.
 *
 * `open` is the hydration signal every other section takes. Here it means "read
 * the stack again": the shell pushes the `mainSyncFolder` fault after the sync
 * engine opens, and the folder tier's faults change as profiles are read, so
 * the answer is not the same one it was at boot.
 */
export function ConfigSourceSection({ open }: { open: boolean }) {
  const layers = useConfigLayersStore((state) => state.layers);
  const error = useConfigLayersStore((state) => state.error);

  useEffect(() => {
    if (!open) {
      return;
    }
    void refreshConfigLayers();
  }, [open]);

  // Before the first read resolves there is nothing true to say. A "no settings
  // file" line here would be a claim the frontend has not yet earned.
  if (layers === null) {
    return error === null ? null : (
      <div className="mt-1 flex flex-col gap-2 border-border border-t pt-3 text-sm">
        <p className="font-medium">{CONFIG_SOURCE_TITLE}</p>
        <p className="text-destructive text-xs">{CONFIG_LAYERS_UNREADABLE}</p>
      </div>
    );
  }

  return (
    <div className="mt-1 flex min-w-0 flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">{CONFIG_SOURCE_TITLE}</p>
      {/* Rust-composed, rendered verbatim: the counts and the consequence are
          one sentence, asserted in `keeper-core`'s own tests, so this surface
          and the log cannot word the same state two different ways. */}
      <p className="text-muted-foreground text-xs">{layers.summary}</p>
      {layers.mainFolder !== null && (
        <p className="min-w-0 break-all text-muted-foreground text-xs">
          {CONFIG_SOURCE_MAIN_FOLDER_LABEL}: <span className="font-mono">{layers.mainFolder}</span>
        </p>
      )}
      {/* Faults first. A key that loaded from the wrong file is a curiosity; a
          file that did not load at all is the thing someone has to go and fix,
          and burying it under a list would be the same silence this section
          exists to end. */}
      {layers.faults.length > 0 && (
        <div className="flex min-w-0 flex-col gap-1">
          <p className="font-medium text-destructive text-xs">{CONFIG_SOURCE_FAULTS_TITLE}</p>
          <ul className="flex min-w-0 flex-col gap-1">
            {layers.faults.map((fault) => (
              <li
                key={`${fault.kind}:${fault.path}:${fault.summary}`}
                // A sentence Rust wrote about a file, not the file's own path —
                // so it is set in the room's voice. The path inside it is
                // already quoted by the sentence.
                className="min-w-0 break-all text-destructive text-xs"
              >
                {fault.summary}
              </li>
            ))}
          </ul>
        </div>
      )}
      {layers.overrides.length > 0 && (
        <div className="flex min-w-0 flex-col gap-1">
          <p className="font-medium text-xs">{CONFIG_SOURCE_OVERRIDES_TITLE}</p>
          <ul className="flex min-w-0 flex-col gap-1">
            {layers.overrides.map((entry) => (
              <li key={entry.key} className="flex min-w-0 flex-col">
                {/* The key verbatim, because it is what the user types into the
                    file. A prettified label would be unsearchable in the one
                    document this list is about. */}
                <span className="font-mono text-xs">{entry.key}</span>
                <span className="text-muted-foreground text-xs">{entry.source}</span>
                <span className="min-w-0 break-all font-mono text-meta text-muted-foreground">
                  {entry.path}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {/* Beneath a list that may be stale rather than instead of it: a failed
          re-read leaves the last good answer on screen, which is more useful
          than a blank section, as long as it admits it may be behind. */}
      {error !== null && <p className="text-destructive text-xs">{error}</p>}
    </div>
  );
}
