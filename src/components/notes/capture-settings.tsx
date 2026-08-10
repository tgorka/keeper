/**
 * Settings → Quick capture: the template a capture starts from, and the tag its
 * notes carry (Story 45.16, FR-193).
 *
 * Two knobs and one computed sentence, and the sentence is why this is a
 * surface rather than a pair of fields.
 *
 * **A capture tag costs something, and keeper says what.** Story 44.3 seeds
 * Inbox as `is:untagged`, so the moment captures carry a tag they stop being
 * unfiled and leave the one space designed to receive them — which is exactly
 * why 44.7's shipped templates add no tags of their own. That cost is computed,
 * never asserted: `notes_capture_impact` runs each of this vault's real stored
 * queries over the note a capture would write and hands back finished
 * sentences. A hardcoded line about Inbox would be wrong for a vault whose
 * Inbox has been edited and silent for the space the user wrote themselves
 * (AD-55, AD-58).
 *
 * **Nothing renders without a vault.** A template lives in a vault and a tag
 * files a note in one, so with no vault flagged there is nothing here to set —
 * absent rather than a section of disabled controls, the same rule the
 * Recording and Sync sections follow for a capability they lack.
 */
import { useEffect, useId, useState } from "react";
import { NO_TEMPLATE, TemplateSelect } from "@/components/notes/template-select";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { NoteTemplateVm, NoteVaultSettingsReq, NoteVaultVm } from "@/lib/ipc/client";
import { notesCaptureImpact, notesTemplates, notesVaultSettingsSave } from "@/lib/ipc/client";
import {
  ensureNotesVaultsHydrated,
  notesVaultsStore,
  useNotesVaultsStore,
} from "@/lib/stores/notes-vaults";

/** The section heading, so the dialog and its test cannot disagree about it. */
export const CAPTURE_SECTION_TITLE = "Quick capture";

/**
 * The standing explanation of what the tag is for, shown whether or not one is
 * set.
 *
 * It names the trade in both directions, because a field whose only annotation
 * is a warning reads as a field you should not touch — and this one is how a
 * person gets a Captures space at all.
 */
export const CAPTURE_TAG_NOTE =
  "Every quick capture gets this tag, so a space can select captures with tag: instead of a folder. keeper folds it the way it folds every tag — lower case, spaces to hyphens. Leave it empty and captures carry no tag.";

/**
 * What the chooser says under itself when nothing is chosen.
 *
 * It names the fall-through explicitly: with no capture template the vault's
 * default template still applies, and someone who reads "No template" and then
 * gets a templated capture would reasonably call that a bug.
 */
export const CAPTURE_TEMPLATE_NOTE =
  "A template is a note tagged template. With none chosen here, a capture starts from the vault's default template if there is one.";

/** The red line under the chooser when the stored template is not in the vault. */
export const CAPTURE_TEMPLATE_MISSING =
  "This names a template that isn't in the vault. Captures are still created, just without it — pick another, or restore the template.";

/** The heading above the computed consequence list. */
export const CAPTURE_IMPACT_TITLE = "With this tag, captures will no longer appear in:";

/** What the user is told when a save did not land. */
export const CAPTURE_SAVE_FAILED = "keeper couldn't save this. Nothing has been changed.";

/** One vault's quick-capture settings, or nothing when there is no vault. */
export function CaptureSettingsSection({ open }: { open: boolean }) {
  const vaults = useNotesVaultsStore((state) => state.vaults);
  const activeVaultId = useNotesVaultsStore((state) => state.activeVaultId);

  useEffect(() => {
    if (open) {
      void ensureNotesVaultsHydrated();
    }
  }, [open]);

  // The active vault, else the only one there is. A vault becomes active when
  // the user opens Notes, and somebody who flagged one folder and went straight
  // to Settings has not done that yet — refusing to render for them would hide
  // the setting behind an unrelated act. With several vaults and none active
  // there is no honest answer to "which vault's captures", so nothing renders.
  const vault =
    vaults?.find((candidate) => candidate.id === activeVaultId) ??
    (vaults?.length === 1 ? vaults[0] : undefined);
  if (vault === undefined) {
    return null;
  }
  // Keyed on the vault so switching vaults remounts, rather than editing one
  // vault's tag into another's form.
  return <CaptureSettingsForm key={vault.id} vault={vault} />;
}

function CaptureSettingsForm({ vault }: { vault: NoteVaultVm }) {
  const templateFieldId = useId();
  const tagFieldId = useId();
  // The stored values, verbatim. Seeded from the VM rather than resolved
  // against the template list, so a template that is missing right now stays
  // selected and stays visible instead of being cleared by a render.
  const [template, setTemplate] = useState(vault.captureTemplate ?? NO_TEMPLATE);
  const [tag, setTag] = useState(vault.captureTag ?? "");
  const [choices, setChoices] = useState<readonly NoteTemplateVm[]>([]);
  const [choicesLoaded, setChoicesLoaded] = useState(false);
  const [impact, setImpact] = useState<readonly string[]>([]);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void notesTemplates(vault.id)
      .then((found) => {
        if (!cancelled) {
          setChoices(found);
          setChoicesLoaded(true);
        }
      })
      .catch(() => {
        // A list that will not load leaves the chooser with nothing to browse.
        // The stored value is still rendered and still saved: a failed read is
        // not evidence that the configured template is gone.
      });
    return () => {
      cancelled = true;
    };
  }, [vault.id]);

  // The consequence, recomputed for the tag the form is HOLDING rather than the
  // one on disk, so the answer arrives before Save rather than after it.
  useEffect(() => {
    let cancelled = false;
    const asked = tag.trim();
    void notesCaptureImpact(vault.id, asked === "" ? null : asked)
      .then((sentences) => {
        if (!cancelled) {
          setImpact(sentences);
        }
      })
      .catch(() => {
        // keeper could not work out the consequence. Showing none is the only
        // honest option — an invented sentence about Inbox would be a claim
        // about a query nobody evaluated.
        if (!cancelled) {
          setImpact([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [vault.id, tag]);

  /**
   * Persist exactly one knob.
   *
   * Every other field goes as `null` — "the caller did not express this" — so a
   * template change cannot carry a half-typed tag with it, which is what a
   * whole-form save would do (AD-34-9).
   */
  const save = (patch: Partial<NoteVaultSettingsReq>) => {
    const settings: NoteVaultSettingsReq = {
      subfolder: null,
      journalTemplate: null,
      defaultTemplate: null,
      captureTemplate: null,
      captureTag: null,
      cadence: null,
      ...patch,
    };
    void notesVaultSettingsSave(vault.id, settings)
      .then((saved) => {
        setFailure(null);
        // Mirror what is actually in force: Rust folds the tag and refuses the
        // `template` marker, so the field must show what was stored rather than
        // what was typed (AD-34-8).
        //
        // **Only over the value this save sent.** The field has two producers
        // and they are sequenced rather than racing: blur, then keep typing
        // while the write is in flight, and this response would land on top of
        // the keystrokes made since — silently, and with a value that looks
        // authoritative because it came from Rust. A functional update reads
        // what is on screen now and declines to touch anything newer.
        setTemplate((live) =>
          live === (settings.captureTemplate ?? live)
            ? (saved.captureTemplate ?? NO_TEMPLATE)
            : live,
        );
        setTag((live) =>
          live === (settings.captureTag ?? live) ? (saved.captureTag ?? "") : live,
        );
        // Mirror into the shared vault list too, so the next surface to read it
        // shows the tag that is in force rather than the one from before this
        // save. Guarded rather than `?? []`: a null mirror means "keeper has
        // not looked yet", and turning that into an empty list would replace
        // "unknown" with "you have no vaults" — from a form that is only on
        // screen because a vault was in the list a moment ago.
        const state = notesVaultsStore.getState();
        if (state.vaults !== null) {
          state.setVaults(
            state.vaults.map((candidate) => (candidate.id === saved.id ? saved : candidate)),
          );
        }
      })
      .catch(() => {
        setFailure(CAPTURE_SAVE_FAILED);
      });
  };

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">{CAPTURE_SECTION_TITLE}</p>

      <div className="flex flex-col gap-1.5">
        <Label htmlFor={templateFieldId}>Captures start from</Label>
        <TemplateSelect
          id={templateFieldId}
          value={template}
          choices={choices}
          loaded={choicesLoaded}
          onChange={(path) => {
            setTemplate(path);
            save({ captureTemplate: path });
          }}
          missingSentence={CAPTURE_TEMPLATE_MISSING}
          note={CAPTURE_TEMPLATE_NOTE}
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <Label htmlFor={tagFieldId}>Tag every capture with</Label>
        <Input
          id={tagFieldId}
          value={tag}
          placeholder="capture"
          onChange={(event) => setTag(event.target.value)}
          // On blur, not on every keystroke: the tag is a profile write that
          // goes through the sync engine, and `captur`, `capture ` and
          // `capture` would be three of them for one edit.
          onBlur={() => save({ captureTag: tag })}
        />
        <p className="text-muted-foreground text-sm">{CAPTURE_TAG_NOTE}</p>
      </div>

      {impact.length > 0 && (
        <div data-slot="capture-impact" className="flex flex-col gap-1">
          <p className="text-held text-sm">{CAPTURE_IMPACT_TITLE}</p>
          <ul className="flex list-disc flex-col gap-1 pl-5 text-muted-foreground text-sm">
            {impact.map((sentence) => (
              <li key={sentence}>{sentence}</li>
            ))}
          </ul>
        </div>
      )}

      {failure !== null && (
        <p data-slot="capture-settings-error" className="text-destructive text-sm">
          {failure}
        </p>
      )}
    </div>
  );
}
