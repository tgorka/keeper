/**
 * Settings as categories instead of one long scroll (Epic 102, Story 102.8).
 *
 * Two arrangements of the same sections:
 *
 * - **rail** — the desktop pane: the category list on the left, the chosen
 *   category's sections on the right, scrolling on their own so the list stays
 *   put.
 * - **drill** — the phone and the dialog: the category list, and a tap opens
 *   one category with a back button to the list. The caller owns the scroll.
 *
 * Every section stays mounted whatever is chosen, hidden when its category is
 * not showing. That keeps what each section already does — it reads its state
 * from Rust when `open` flips — exactly as it was, and it is how the list knows
 * which categories have anything in them: several sections render themselves
 * away (Quick capture without a vault, Sessions without a zone, Default agent
 * without an agent), so a category is listed only while one of its sections
 * rendered something, never as a link to an empty page.
 *
 * Under each category, what of it is this device's own: what applies only to
 * this device but is backed up in the account, and what is kept only on this
 * device (`@/lib/settings-device-notes`). The rest of Settings follows the
 * person's organisation account (`settings_sync.rs`).
 *
 * On the phone the drill-in is a sub-level of the shell's stack: the shell
 * owns the chosen category ({@link SettingsDrillOwner}), so its one back — the
 * header's button, the edge swipe, Escape — returns a category to the list
 * before it leaves Settings, and the list draws no back of its own.
 */
import { ChevronLeft, ChevronRight } from "lucide-react";
import type { ReactNode, RefObject } from "react";
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { useStore } from "zustand";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  categoryOf,
  SETTINGS_CATEGORIES,
  type SettingsCategoryId,
  type SettingsSectionId,
} from "@/lib/settings-categories";
import {
  DEVICE_FILE_TITLE,
  DEVICE_ONLY_TITLE,
  type DeviceNote,
  type DeviceNoteKind,
  kindOf,
} from "@/lib/settings-device-notes";
import { type SettingsReveal, settingsUiStore } from "@/lib/stores/settings-ui";
import { cn } from "@/lib/utils";

/** One section as a category renders it. */
export interface SettingsSectionSlot {
  node: ReactNode;
  /** What in this section is this device's own, and why; shown under its category. */
  staysHere?: readonly DeviceNote[];
}

/**
 * Every section, `null` where this build cannot have it (its capability is
 * off). A `Record`, so a section added to the category table and forgotten
 * here — or the reverse — is a type error rather than a setting nobody can reach.
 */
export type SettingsSections = Readonly<Record<SettingsSectionId, SettingsSectionSlot | null>>;

export type SettingsLayout = "rail" | "drill";

/** The category list's accessible name. */
export const SETTINGS_CATEGORIES_LABEL = "Settings categories";

/** The drill-in back button: it returns to the category list. */
export const SETTINGS_BACK_LABEL = "All settings";

/**
 * A drill-in whose back belongs to the phone shell's stack: the shell holds
 * the chosen category and pops it with the stack's own back, and `backRef` is
 * that back's button, which takes focus when a person drills in.
 */
export interface SettingsDrillOwner {
  category: SettingsCategoryId | null;
  onCategoryChange: (category: SettingsCategoryId | null) => void;
  backRef: RefObject<HTMLButtonElement | null>;
}

const NOTE_KINDS: readonly { kind: DeviceNoteKind; title: string }[] = [
  { kind: "account", title: DEVICE_FILE_TITLE },
  { kind: "device", title: DEVICE_ONLY_TITLE },
];

/**
 * What Settings says about where it all lives, under its heading. True with no
 * account too: it says what an account does rather than claiming one is there.
 */
export const SETTINGS_TRAVEL_SENTENCE =
  "With an organisation account signed in, these settings follow you to every device you use. What applies only to this device, or is kept only on it, says so where it sits.";

/**
 * How long a revealed section is held in place while the sections above it
 * fill in: the hold ends after {@link REVEAL_SETTLE_QUIET_MS} with no change
 * in their size, after {@link REVEAL_SETTLE_LIMIT_MS} whatever happens, or the
 * moment the person scrolls, clicks, touches or types — whichever comes first.
 */
export const REVEAL_SETTLE_QUIET_MS = 1000;
export const REVEAL_SETTLE_LIMIT_MS = 3000;

/** What a person does that ends a hold: their scroll or click wins over keeper's. */
const PERSON_EVENTS = ["wheel", "touchmove", "pointerdown", "keydown"] as const;

export function SettingsCategories({
  layout,
  sections,
  owner,
}: {
  layout: SettingsLayout;
  sections: SettingsSections;
  /** The phone shell, when it owns the drill-in's back (`drill` only). */
  owner?: SettingsDrillOwner;
}) {
  const uid = useId();
  const panels = useRef<HTMLDivElement | null>(null);
  const ownBack = useRef<HTMLButtonElement | null>(null);
  const back = owner?.backRef ?? ownBack;
  const rows = useRef(new Map<SettingsCategoryId, HTMLButtonElement>());
  // Whether the next change of category should focus the back button: a
  // person drilled in. A ref, because focus follows the render that shows the
  // element, not the click.
  const focusBack = useRef(false);
  const reveal = useStore(settingsUiStore, (s) => s.reveal);
  const [pending, setPending] = useState<SettingsReveal | null>(reveal);
  // The reveal whose place is held while the layout settles; outlives
  // `pending`, which ends when the reveal lands, not when the layout stops.
  const [holding, setHolding] = useState<SettingsReveal | null>(reveal);
  const [ownSelected, setOwnSelected] = useState<SettingsCategoryId | null>(
    reveal ? categoryOf(reveal.section) : null,
  );
  const selected = owner ? owner.category : ownSelected;
  const setSelected = owner?.onCategoryChange ?? setOwnSelected;
  // Which sections rendered something, measured from the DOM; `null` until the
  // first measurement, when every present section counts.
  const [rendered, setRendered] = useState<ReadonlySet<SettingsSectionId> | null>(null);

  useLayoutEffect(() => {
    const root = panels.current;
    if (!root) {
      return;
    }
    const measure = () => {
      const next = new Set<SettingsSectionId>();
      for (const category of SETTINGS_CATEGORIES) {
        for (const section of category.sections) {
          if (root.querySelector(`[data-settings-section="${section}"]:not(:empty)`)) {
            next.add(section);
          }
        }
      }
      setRendered((prev) =>
        prev && prev.size === next.size && [...next].every((id) => prev.has(id)) ? prev : next,
      );
    };
    measure();
    const observer = new MutationObserver(measure);
    observer.observe(root, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);

  const showsSomething = (section: SettingsSectionId) =>
    sections[section] !== null && (rendered === null || rendered.has(section));
  const listed = SETTINGS_CATEGORIES.filter((category) => category.sections.some(showsSomething));
  // The rail always shows a category — the chosen one while it has anything,
  // else the first. The drill shows the list until a category is chosen, and
  // goes back to it if the chosen one empties under the person.
  const chosen = listed.find((category) => category.id === selected);
  const shown = chosen ?? (layout === "rail" ? listed[0] : undefined);

  useEffect(() => {
    if (!reveal) {
      return;
    }
    setSelected(categoryOf(reveal.section));
    setPending(reveal);
    setHolding(reveal);
    settingsUiStore.getState().clearReveal();
  }, [reveal, setSelected]);

  // Land a reveal once its category is the one showing: scroll its section
  // into view at once, then focus the element it names once that can take
  // focus — a section that reads from Rust draws its fields a moment after it
  // mounts, or draws them disabled until the read answers. The request stays
  // until the element really has focus, or a person chooses another category.
  useEffect(() => {
    const root = panels.current;
    if (!pending || !root || shown?.id !== categoryOf(pending.section)) {
      return;
    }
    root
      .querySelector(`[data-settings-section="${pending.section}"]`)
      ?.scrollIntoView?.({ block: "start" });
    const focus = () => {
      if (pending.focusId === null) {
        return true;
      }
      const target = root.querySelector<HTMLElement>(`[id="${pending.focusId}"]`);
      if (!target) {
        return false;
      }
      target.focus();
      if (target.ownerDocument.activeElement !== target) {
        return false;
      }
      target.scrollIntoView?.({ block: "center" });
      return true;
    };
    if (focus()) {
      setPending(null);
      return;
    }
    const observer = new MutationObserver(() => {
      if (focus()) {
        observer.disconnect();
        setPending(null);
      }
    });
    // Attributes too: a field that is already there turns focusable by losing
    // `disabled` (or `hidden`, `inert`), not by being inserted.
    observer.observe(root, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["disabled", "hidden", "inert", "tabindex"],
    });
    return () => observer.disconnect();
  }, [pending, shown?.id]);

  // Hold a reveal's place until the layout settles. The sections above the
  // revealed one read from Rust after they mount and grow when the answer
  // comes, which pushes the revealed one down after the scroll above landed —
  // Always allowed… ended a screen below the fold (R602-01). So every change
  // in their size scrolls it back: the section's top, or the revealed field
  // once it has focus. Scroll anchoring cannot do this: it keeps nothing in
  // place at a scroll offset of zero, which is where a fresh Settings opens.
  useEffect(() => {
    const root = panels.current;
    if (
      !holding ||
      !root ||
      shown?.id !== categoryOf(holding.section) ||
      typeof ResizeObserver === "undefined"
    ) {
      return;
    }
    const section = root.querySelector<HTMLElement>(`[data-settings-section="${holding.section}"]`);
    const category = section?.parentElement;
    if (!section || !category) {
      return;
    }
    const above: Element[] = [];
    for (const sibling of category.querySelectorAll(":scope > [data-settings-section]")) {
      if (sibling === section) {
        break;
      }
      above.push(sibling);
    }
    const place = () => {
      const field =
        holding.focusId === null
          ? null
          : root.querySelector<HTMLElement>(`[id="${holding.focusId}"]`);
      if (field && field.ownerDocument.activeElement === field) {
        field.scrollIntoView?.({ block: "center" });
      } else {
        section.scrollIntoView?.({ block: "start" });
      }
    };
    const release = () => setHolding(null);
    let quiet = window.setTimeout(release, REVEAL_SETTLE_QUIET_MS);
    const limit = window.setTimeout(release, REVEAL_SETTLE_LIMIT_MS);
    const observer = new ResizeObserver(() => {
      place();
      window.clearTimeout(quiet);
      quiet = window.setTimeout(release, REVEAL_SETTLE_QUIET_MS);
    });
    for (const element of above) {
      observer.observe(element);
    }
    const doc = root.ownerDocument;
    for (const type of PERSON_EVENTS) {
      doc.addEventListener(type, release, { capture: true, passive: true });
    }
    return () => {
      observer.disconnect();
      window.clearTimeout(quiet);
      window.clearTimeout(limit);
      for (const type of PERSON_EVENTS) {
        doc.removeEventListener(type, release, { capture: true });
      }
    };
  }, [holding, shown?.id]);

  // Focus follows the category that is showing: drilling in focuses the back
  // (the list's own, or the shell's), and coming back to the list — by
  // whichever back — focuses the row the person came from.
  const lastShown = useRef(shown?.id);
  // biome-ignore lint/correctness/useExhaustiveDependencies: focus follows the category that is showing, the one thing this effect answers to
  useEffect(() => {
    const was = lastShown.current;
    lastShown.current = shown?.id;
    if (focusBack.current) {
      focusBack.current = false;
      back.current?.focus();
    } else if (layout === "drill" && shown === undefined && was !== undefined) {
      rows.current.get(was)?.focus();
    }
  }, [shown?.id]);

  const choose = (id: SettingsCategoryId) => {
    // A person's choice wins over a reveal still waiting for its field.
    setPending(null);
    setHolding(null);
    focusBack.current = layout === "drill";
    setSelected(id);
    if (layout === "rail") {
      panels.current?.scrollIntoView?.({ block: "start" });
    }
  };

  const list = (
    <nav aria-label={SETTINGS_CATEGORIES_LABEL}>
      <ul className="flex flex-col gap-0.5">
        {listed.map((category) => {
          const active = layout === "rail" && category.id === shown?.id;
          return (
            <li key={category.id}>
              <Button
                ref={(element) => {
                  if (element) {
                    rows.current.set(category.id, element);
                  } else {
                    rows.current.delete(category.id);
                  }
                }}
                type="button"
                variant="ghost"
                aria-current={active ? "page" : undefined}
                className={cn(
                  "w-full justify-start gap-2 whitespace-normal text-left",
                  layout === "drill" && "min-h-11 justify-between",
                  active && "bg-accent text-accent-foreground",
                )}
                onClick={() => choose(category.id)}
              >
                {category.title}
                {layout === "drill" && <ChevronRight aria-hidden="true" />}
              </Button>
            </li>
          );
        })}
      </ul>
    </nav>
  );

  const body = (
    <div ref={panels} className="flex min-w-0 flex-col gap-4">
      {/* A category this build has no section of is not rendered at all; one
          whose sections are there but drew nothing stays mounted, hidden, so
          it can be listed the moment one of them does. */}
      {SETTINGS_CATEGORIES.filter((category) =>
        category.sections.some((section) => sections[section] !== null),
      ).map((category) => {
        const notes = category.sections.flatMap((section) =>
          showsSomething(section) ? (sections[section]?.staysHere ?? []) : [],
        );
        return (
          <section
            key={category.id}
            data-settings-category={category.id}
            aria-labelledby={`${uid}-${category.id}`}
            hidden={category.id !== shown?.id}
            className="flex min-w-0 flex-col gap-4"
          >
            <h2 id={`${uid}-${category.id}`} className="font-heading text-title">
              {category.title}
            </h2>
            {category.sections.map((section) => {
              const slot = sections[section];
              return slot === null ? null : (
                <div key={section} data-settings-section={section} className="min-w-0 empty:hidden">
                  {slot.node}
                </div>
              );
            })}
            {notes.length > 0 && (
              <footer className="flex flex-col gap-3 border-border border-t pt-3 text-muted-foreground text-xs">
                {NOTE_KINDS.map(({ kind, title }) => {
                  const said = notes.filter((note) => kindOf(note) === kind);
                  return said.length === 0 ? null : (
                    <div key={kind} className="flex flex-col gap-1">
                      <p className="font-medium text-foreground">{title}</p>
                      <ul className="flex list-disc flex-col gap-1 pl-4">
                        {said.map((note) => (
                          <li key={note.text}>{note.text}</li>
                        ))}
                      </ul>
                    </div>
                  );
                })}
              </footer>
            )}
          </section>
        );
      })}
    </div>
  );

  if (layout === "rail") {
    return (
      <div className="flex min-h-0 min-w-0 flex-1">
        <div className="w-52 shrink-0 overflow-y-auto border-border border-r p-2 last:border-r-0">
          {list}
        </div>
        <ScrollArea className="min-h-0 flex-1" fitWidth>
          <div className="p-4">{body}</div>
        </ScrollArea>
      </div>
    );
  }
  return (
    <div className="flex min-w-0 flex-col gap-4">
      {shown === undefined ? (
        list
      ) : owner ? null : (
        <Button
          ref={ownBack}
          type="button"
          variant="ghost"
          className="min-h-11 self-start"
          onClick={() => setSelected(null)}
        >
          <ChevronLeft aria-hidden="true" />
          {SETTINGS_BACK_LABEL}
        </Button>
      )}
      {body}
    </div>
  );
}
