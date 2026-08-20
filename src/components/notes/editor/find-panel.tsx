/**
 * The in-note find bar (Story 55.2, FR-267).
 *
 * # What this replaces, and what it deliberately does not
 *
 * `search()` ships a panel and this repo had never styled it, so `⌘F` in a note
 * opened the browser's own controls: two rows split by a `<br>`, native
 * checkboxes labelled "match case / regexp / by word", four buttons reading
 * "next / previous / all / replace all" in lowercase, and a `×` absolutely
 * positioned into the corner. Six inches below it sat the note list's filter
 * bar — the same job, done in the app's own vocabulary.
 *
 * So this is a **presentation swap and nothing else**. Every command here is
 * `@codemirror/search`'s own export, the query lives in CodeMirror's state
 * where it always did, and the keymap is untouched. If you are looking for the
 * matching logic, it is not in this file and must never be: a second
 * implementation of "what counts as a match" is how a regexp toggle starts
 * meaning two different things in one editor.
 *
 * # Why the query is mirrored into React state at all
 *
 * A controlled `Input` needs a value this render can see, and
 * `getSearchQuery(view.state)` is only readable at dispatch time. So the fields
 * are React state, and the two are kept in step in both directions: typing
 * dispatches `setSearchQuery`, and {@link createFindPanel}'s `update` pushes
 * changes that came from anywhere else — `selectSelectionMatches`, another
 * panel, a future command — back into the fields. The dispatch is guarded by
 * `SearchQuery.eq`, so the round trip stops rather than ringing.
 *
 * # Why the buttons cancel their own mousedown
 *
 * The same reason `format-toolbar.tsx` does, one layer in. Clicking "next"
 * moves DOM focus to the button, so the next thing typed goes nowhere — with
 * the stock panel you click through matches and then have to click back into
 * the field. Cancelling `mousedown` on the buttons only (never on the two
 * fields, which must be clickable) keeps the caret where the user left it.
 */
import {
  closeSearchPanel,
  findNext,
  findPrevious,
  getSearchQuery,
  replaceAll,
  replaceNext,
  SearchQuery,
  search,
  selectMatches,
  setSearchQuery,
} from "@codemirror/search";
import type { Extension } from "@codemirror/state";
import type { EditorView, Panel } from "@codemirror/view";
import { runScopeHandlers, EditorView as View } from "@codemirror/view";
import {
  ArrowDown,
  ArrowUp,
  CaseSensitive,
  ChevronRight,
  Regex,
  Search,
  TextSelect,
  WholeWord,
  X,
} from "lucide-react";
import {
  type KeyboardEvent,
  type MouseEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { createRoot } from "react-dom/client";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

/** The three flags, as the query carries them. */
interface Flags {
  readonly caseSensitive: boolean;
  readonly regexp: boolean;
  readonly wholeWord: boolean;
}

interface FindBarProps {
  readonly view: EditorView;
  /** Register for queries that arrived from outside this bar. Returns its own
   *  unsubscribe, so the effect can hand it straight back. */
  readonly onExternalQuery: (listen: (query: SearchQuery) => void) => () => void;
}

/** `aria-pressed:` styling, lifted verbatim from `note-filter-bar.tsx` so the
 *  two bars stay one design rather than two that resemble each other. */
const MODE_CLASS =
  "shrink-0 text-muted-foreground aria-pressed:bg-accent aria-pressed:text-accent-foreground";

/** Buttons must not take the caret; see the header. */
const keepFocus = (event: MouseEvent): void => {
  event.preventDefault();
};

function FindBar({ view, onExternalQuery }: FindBarProps): React.ReactElement {
  const initial = getSearchQuery(view.state);
  const [search, setSearch] = useState(initial.search);
  const [replace, setReplace] = useState(initial.replace);
  const [flags, setFlags] = useState<Flags>({
    caseSensitive: initial.caseSensitive,
    regexp: initial.regexp,
    wholeWord: initial.wholeWord,
  });
  // Collapsed by default. The stock panel showed both rows always, which is
  // half of why it read as a form: most `⌘F` presses are a find, and a replace
  // field nobody asked for is a second thing to tab past.
  const [replacing, setReplacing] = useState(false);
  const readOnly = view.state.readOnly;

  // The panel's `mount()` is the wrong hook for this, and the stock panel could
  // use it only because its DOM was built synchronously. React commits after
  // CodeMirror has mounted the element, so a `select()` from there runs against
  // a field that does not exist yet — the panel opened without the caret, which
  // no amount of looking at it revealed and one test did.
  //
  // Focused *and* selected, in that order. The stock panel called `select()`
  // alone and got focus as a browser side effect that the spec does not
  // promise — jsdom, correctly, does not do it. Selected because reopening
  // `⌘F` over an old query should be replaced by typing, not appended to.
  const fieldRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    fieldRef.current?.focus();
    fieldRef.current?.select();
  }, []);

  const dispatch = useCallback(
    (next: { search?: string; replace?: string } & Partial<Flags>) => {
      const query = new SearchQuery({
        search: next.search ?? search,
        replace: next.replace ?? replace,
        caseSensitive: next.caseSensitive ?? flags.caseSensitive,
        regexp: next.regexp ?? flags.regexp,
        wholeWord: next.wholeWord ?? flags.wholeWord,
      });
      if (!query.eq(getSearchQuery(view.state))) {
        view.dispatch({ effects: setSearchQuery.of(query) });
      }
    },
    [flags, replace, search, view],
  );

  useEffect(
    () =>
      onExternalQuery((query) => {
        setSearch(query.search);
        setReplace(query.replace);
        setFlags({
          caseSensitive: query.caseSensitive,
          regexp: query.regexp,
          wholeWord: query.wholeWord,
        });
      }),
    [onExternalQuery],
  );

  const toggle = (key: keyof Flags) => () => {
    const next = { ...flags, [key]: !flags[key] };
    setFlags(next);
    dispatch(next);
  };

  const keydown = (event: KeyboardEvent<HTMLDivElement>): void => {
    // First refusal to `searchKeymap`, exactly as the stock panel gave it, so
    // every binding that worked inside the old panel still works inside this
    // one — including the `⌘F` that opened it.
    if (runScopeHandlers(view, event.nativeEvent, "search-panel")) {
      event.preventDefault();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      closeSearchPanel(view);
      return;
    }
    if (event.key !== "Enter") {
      return;
    }
    event.preventDefault();
    const target = event.target as HTMLElement;
    if (target.getAttribute("name") === "replace") {
      replaceNext(view);
      return;
    }
    (event.shiftKey ? findPrevious : findNext)(view);
  };

  return (
    // A `search` landmark, because that is what this is — and the panel is a
    // keyboard scope, so `onKeyDown` has to sit on the element containing every
    // control.
    <>
      {/* biome-ignore lint/a11y/useSemanticElements: `<search>` says this in one
          word and shipped in Safari 17 — macOS 14. Tailwind v4 already puts this
          app's real floor around Safari 16.4, so the band between them is live
          machines on which an unknown element maps to `generic`, costing the
          landmark and its name. The explicit role holds on both sides of it. */}
      <div
        role="search"
        aria-label="Find in note"
        className="flex flex-col gap-1.5 border-border border-b bg-background px-3 py-2"
        onKeyDown={keydown}
      >
        {/* Wrapping, with a floor under the field. The note panel's own floor
            is 280px since Story 55.1, and at that width nine controls in one
            row squeeze the find field to 22px — a field you cannot read what
            you typed in. The groups drop to a second line instead; two rows of
            usable controls beat one row of unusable ones. */}
        <div className="flex flex-wrap items-center gap-1.5">
          {readOnly ? null : (
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label={replacing ? "Hide replace" : "Show replace"}
              title={replacing ? "Hide replace" : "Show replace"}
              aria-expanded={replacing}
              className="shrink-0 text-muted-foreground"
              onMouseDown={keepFocus}
              onClick={() => setReplacing((open) => !open)}
            >
              <ChevronRight
                aria-hidden="true"
                className={cn("size-3 transition-transform", replacing && "rotate-90")}
              />
            </Button>
          )}
          <div className="flex min-w-[9rem] flex-1 items-center gap-1.5">
            <Search aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
            <Input
              ref={fieldRef}
              name="search"
              aria-label="Find"
              placeholder="Find"
              className="h-8 min-w-0 flex-1"
              value={search}
              onChange={(event) => {
                setSearch(event.target.value);
                dispatch({ search: event.target.value });
              }}
            />
          </div>
          <div className="flex shrink-0 items-center gap-0.5">
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label="Match case"
              title="Match case"
              aria-pressed={flags.caseSensitive}
              className={MODE_CLASS}
              onMouseDown={keepFocus}
              onClick={toggle("caseSensitive")}
            >
              <CaseSensitive aria-hidden="true" className="size-3" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label="Regular expression"
              title="Regular expression"
              aria-pressed={flags.regexp}
              className={MODE_CLASS}
              onMouseDown={keepFocus}
              onClick={toggle("regexp")}
            >
              <Regex aria-hidden="true" className="size-3" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label="Whole word"
              title="Whole word"
              aria-pressed={flags.wholeWord}
              className={MODE_CLASS}
              onMouseDown={keepFocus}
              onClick={toggle("wholeWord")}
            >
              <WholeWord aria-hidden="true" className="size-3" />
            </Button>
          </div>
          <div className="flex shrink-0 items-center gap-0.5">
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label="Previous match"
              title="Previous match"
              className="shrink-0 text-muted-foreground"
              onMouseDown={keepFocus}
              onClick={() => findPrevious(view)}
            >
              <ArrowUp aria-hidden="true" className="size-3" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label="Next match"
              title="Next match"
              className="shrink-0 text-muted-foreground"
              onMouseDown={keepFocus}
              onClick={() => findNext(view)}
            >
              <ArrowDown aria-hidden="true" className="size-3" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label="Select all matches"
              title="Select all matches"
              className="shrink-0 text-muted-foreground"
              onMouseDown={keepFocus}
              onClick={() => selectMatches(view)}
            >
              <TextSelect aria-hidden="true" className="size-3" />
            </Button>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label="Close find"
            title="Close find"
            className="ml-auto shrink-0 text-muted-foreground"
            onMouseDown={keepFocus}
            onClick={() => closeSearchPanel(view)}
          >
            <X aria-hidden="true" className="size-3" />
          </Button>
        </div>

        {replacing && !readOnly ? (
          <div className="flex items-center gap-1.5">
            <Input
              name="replace"
              aria-label="Replace"
              placeholder="Replace"
              className="h-8"
              value={replace}
              onChange={(event) => {
                setReplace(event.target.value);
                dispatch({ replace: event.target.value });
              }}
            />
            <Button
              type="button"
              variant="ghost"
              size="xs"
              className="shrink-0"
              onMouseDown={keepFocus}
              onClick={() => replaceNext(view)}
            >
              Replace
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="xs"
              className="shrink-0"
              onMouseDown={keepFocus}
              onClick={() => replaceAll(view)}
            >
              Replace all
            </Button>
          </div>
        ) : null}
      </div>
    </>
  );
}

/**
 * CodeMirror paints the panel container itself, and its defaults are a light
 * theme's: `#f5f5f5` behind the panel and a hardcoded `#ddd` bottom border,
 * both of which survive into dark mode as a grey seam. The bar draws its own
 * background and `border-border` above, so the container's job here is to get
 * out of the way.
 */
export const findPanelTheme = View.theme({
  ".cm-panels": { backgroundColor: "transparent", color: "inherit" },
  ".cm-panels-top": { borderBottom: "none" },
});

/**
 * Above the document, not below it.
 *
 * Stated once, and read twice from here, because it has to be true in two
 * places at once: `search({ top })` positions the *slot*, and the panel object
 * has to answer `top` for itself — `showPanel` asks the panel, not the facet,
 * and a panel that stays silent is placed at the bottom. The stock
 * `SearchPanel` has a `get top()` that forwards the config; this one had
 * neither, and every test still passed while the bar sat under the note.
 */
const PANEL_AT_TOP = true;

/**
 * The whole find bar as one extension: the search state, this panel and the
 * theme that gets CodeMirror's own panel chrome out of its way.
 *
 * Bundled rather than left as three things a caller must remember to combine,
 * because two of them share {@link PANEL_AT_TOP} and the third is meaningless
 * without the other two.
 */
export function findBar(): Extension {
  return [search({ top: PANEL_AT_TOP, createPanel: createFindPanel }), findPanelTheme];
}

/** The `createPanel` CodeMirror asks for. Exported for tests that want the
 *  panel without the rest of the bundle. */
export function createFindPanel(view: EditorView): Panel {
  const dom = document.createElement("div");
  const root = createRoot(dom);
  const listeners = new Set<(query: SearchQuery) => void>();
  const onExternalQuery = (listen: (query: SearchQuery) => void): (() => void) => {
    listeners.add(listen);
    return () => {
      listeners.delete(listen);
    };
  };

  root.render(<FindBar view={view} onExternalQuery={onExternalQuery} />);

  return {
    dom,
    top: PANEL_AT_TOP,
    // No `mount()`: the one thing it would do — select the find field — belongs
    // to the component, which is the only thing that knows when the field
    // exists. See the effect in `FindBar`.
    update(update) {
      for (const transaction of update.transactions) {
        for (const effect of transaction.effects) {
          if (effect.is(setSearchQuery)) {
            for (const listen of listeners) {
              listen(effect.value);
            }
          }
        }
      }
    },
    destroy() {
      // A microtask, for the reason `file-embed.ts` states: this can run inside
      // a React commit, and unmounting a root mid-render is refused with a
      // warning that leaves the tree attached.
      queueMicrotask(() => {
        root.unmount();
      });
    },
  };
}
