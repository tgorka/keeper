import { X } from "lucide-react";
import {
  type ReactNode,
  type Ref,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { TagSuggest } from "@/components/notes/tag-suggest";
import { matchTags } from "@/components/tags/tag-match";
import { Button } from "@/components/ui/button";
import { Popover, PopoverAnchor } from "@/components/ui/popover";
import { IconHint } from "@/components/ui/tooltip";
import { tagsVocabulary } from "@/lib/ipc/client";
import { type PromptToken, replaceSpan, tokenAtCaret } from "@/lib/notes/prompt-tokens";
import { notesFiltersStore, type TagChip, useNotesFiltersStore } from "@/lib/stores/notes-filters";
import { notesVaultsStore, useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { cn } from "@/lib/utils";

export function SearchField({
  text,
  chips,
  children,
  glyph,
  controls,
  phone,
  searchRef,
  description,
}: {
  text: string;
  chips: readonly TagChip[];
  children: ReactNode;
  glyph: ReactNode;
  /**
   * The bar's own controls, rendered inside the field on physical row 1
   * (FR-619). They are the field's first flow items, so the caret shares
   * their last row when ≥ 96 px remain and starts a full-width row of its
   * own otherwise — the same rule the chips used to be measured by.
   */
  controls: ReactNode;
  phone: boolean;
  searchRef?: Ref<HTMLTextAreaElement>;
  description: string;
}) {
  const id = useId();
  const input = useRef<HTMLTextAreaElement | null>(null);
  const flow = useRef<HTMLDivElement>(null);
  const chipFlow = useRef<HTMLDivElement>(null);
  const controlFlow = useRef<HTMLDivElement>(null);
  const textMeasure = useRef<HTMLSpanElement>(null);
  const [requested, setRequested] = useState(false);
  const vocabularyNonce = useNotesFiltersStore((state) => state.spacesNonce);
  const vaultId = useNotesVaultsStore((state) => state.activeVaultId);
  const [vocabulary, setVocabulary] = useState<readonly string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [token, setToken] = useState<PromptToken | null>(null);
  const [active, setActive] = useState(-1);
  const [multiline, setMultiline] = useState(false);
  const [capped, setCapped] = useState(false);
  const composing = useRef(false);
  const restoreCaret = useRef<{ text: string; caret: number } | null>(null);
  const matches = useMemo(
    () => (token ? matchTags(token.text.replace(/^-/, ""), vocabulary) : []),
    [token, vocabulary],
  );
  const open =
    token !== null &&
    text.slice(token.start, token.end) === token.text &&
    (loading || error !== null || matches.length > 0);
  const attach = useCallback(
    (node: HTMLTextAreaElement | null) => {
      input.current = node;
      if (typeof searchRef === "function") searchRef(node);
      else if (searchRef) searchRef.current = node;
    },
    [searchRef],
  );

  useEffect(() => {
    if (!requested) return;
    let cancelled = false;
    const isCurrent = () =>
      !cancelled &&
      notesVaultsStore.getState().activeVaultId === vaultId &&
      notesFiltersStore.getState().spacesNonce === vocabularyNonce;
    setLoading(true);
    setError(null);
    void tagsVocabulary()
      .then((vm) => {
        if (isCurrent()) setVocabulary(vm.entries.map((entry) => entry.path));
      })
      .catch(() => {
        if (isCurrent())
          setError("Could not load tags. Search and existing filters are still available.");
      })
      .finally(() => {
        if (isCurrent()) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [requested, vocabularyNonce, vaultId]);

  const measure = useCallback(() => {
    const element = input.current;
    if (!element) return;
    element.style.height = "20px";
    const height = element.scrollHeight;
    element.style.height = `${Math.min(60, Math.max(20, height))}px`;
    const width = flow.current?.clientWidth ?? 0;
    const tokens = chipFlow.current;
    if (tokens) {
      // Children are the actual whole-token boxes; offsets are relative to the flow.
      const boxes = Array.from(tokens.children) as HTMLElement[];
      const first = boxes[0];
      const last = boxes[boxes.length - 1];
      const max = phone ? 140 : 80;
      setCapped(
        Boolean(first && last && last.offsetTop + last.offsetHeight - first.offsetTop > max),
      );
    }
    // The caret shares a row with the CONTROLS, not with the chips: since
    // FR-619 the chips follow the text instead of preceding it, so what
    // decides whether the prompt fits inline is the space left after the
    // last control on its row.
    const controlBoxes = Array.from(controlFlow.current?.children ?? []) as HTMLElement[];
    const lastControl = controlBoxes[controlBoxes.length - 1];
    const remaining = lastControl
      ? width - lastControl.offsetLeft - lastControl.offsetWidth - 4
      : width;
    const inlineWidth = remaining >= 96 ? remaining : width;
    setMultiline(text.includes("\n") || (textMeasure.current?.offsetWidth ?? 0) > inlineWidth);
  }, [text, phone]);
  // Measure committed DOM after any token or wrapping change, not just text edits.
  useLayoutEffect(() => measure());
  useLayoutEffect(() => {
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    if (flow.current) observer?.observe(flow.current);
    return () => observer?.disconnect();
  }, [measure]);

  useLayoutEffect(() => {
    const pending = restoreCaret.current;
    if (pending === null || text !== pending.text) return;
    input.current?.focus();
    input.current?.setSelectionRange(pending.caret, pending.caret);
    restoreCaret.current = null;
  }, [text]);
  useEffect(() => {
    if (chips.length === 0) return;
    chipFlow.current?.lastElementChild?.scrollIntoView?.({ block: "nearest" });
  }, [chips.length]);

  function suggest(value: string, caret: number) {
    const next = tokenAtCaret(value, caret);
    setToken(next && next.text !== "-" ? next : null);
    setActive(-1);
  }

  function choose(tag: string, term: "include" | "exclude") {
    if (!token || text.slice(token.start, token.end) !== token.text) return;
    const result = replaceSpan(text, token);
    restoreCaret.current = result;
    const state = notesFiltersStore.getState();
    state.setTagTerm(tag, term);
    state.setText(result.text);
    setToken(null);
    setActive(-1);
  }

  return (
    <Popover
      open={open}
      onOpenChange={(value) => {
        if (!value) setToken(null);
      }}
    >
      <PopoverAnchor asChild>
        <div
          data-slot="search-field"
          className="flex min-w-0 items-start rounded-[5px] border border-input p-2 focus-within:ring-2 focus-within:ring-ring"
        >
          <span
            className={cn(
              "flex w-5 shrink-0 items-center text-muted-foreground",
              phone ? "h-11" : "h-6",
            )}
          >
            {glyph}
          </span>
          <div
            ref={flow}
            className={cn(
              "relative flex min-w-0 flex-1 flex-wrap items-start gap-1",
              phone ? "min-h-11" : "min-h-6",
            )}
          >
            <span
              aria-hidden="true"
              className="pointer-events-none invisible absolute inset-x-0 h-0 overflow-hidden"
            >
              <span ref={textMeasure} className="inline-block whitespace-pre text-sm leading-5">
                {text}
              </span>
            </span>
            <div ref={controlFlow} data-slot="search-controls" className="contents">
              {controls}
            </div>
            <textarea
              ref={attach}
              rows={1}
              role="combobox"
              aria-label="Search notes"
              placeholder="Search notes"
              aria-autocomplete="list"
              aria-expanded={open}
              aria-controls={open ? id : undefined}
              aria-activedescendant={open && active >= 0 ? `${id}-${active}` : undefined}
              aria-describedby={`${id}-description`}
              className={cn(
                "min-w-0 resize-none overflow-x-hidden overflow-y-auto bg-transparent p-0 text-sm leading-5 outline-none placeholder:text-muted-foreground",
                multiline || capped ? "w-full basis-full" : "flex-1 basis-24",
              )}
              style={{ maxHeight: 60, marginTop: multiline || capped ? 0 : phone ? 12 : 2 }}
              value={text}
              onFocus={() => setRequested(true)}
              onClick={(event) => suggest(text, event.currentTarget.selectionStart)}
              onCompositionStart={() => {
                composing.current = true;
                setToken(null);
              }}
              onCompositionEnd={(event) => {
                composing.current = false;
                suggest(event.currentTarget.value, event.currentTarget.selectionStart);
              }}
              onChange={(event) => {
                notesFiltersStore.getState().setText(event.currentTarget.value);
                if (!composing.current)
                  suggest(event.currentTarget.value, event.currentTarget.selectionStart);
              }}
              onKeyDown={(event) => {
                if (composing.current || event.nativeEvent.isComposing) return;
                if (open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
                  if (!matches.length) return;
                  event.preventDefault();
                  setActive((current) =>
                    current < 0
                      ? token?.text.startsWith("-")
                        ? 1
                        : 0
                      : (current + (event.key === "ArrowDown" ? 1 : -1) + matches.length * 2) %
                        (matches.length * 2),
                  );
                } else if (event.key === "ArrowDown" && !open) {
                  if (tokenAtCaret(text, event.currentTarget.selectionStart)) {
                    event.preventDefault();
                    suggest(text, event.currentTarget.selectionStart);
                  }
                } else if (event.key === "Enter" && !event.shiftKey) {
                  // Search is live: Enter accepts/submits, Shift+Enter inserts a hard line break.
                  event.preventDefault();
                  if (open && active >= 0)
                    choose(
                      matches[Math.floor(active / 2)],
                      active % 2 === 0 ? "include" : "exclude",
                    );
                } else if (event.key === "Escape") {
                  event.preventDefault();
                  event.stopPropagation();
                  if (open) {
                    setToken(null);
                    setActive(-1);
                  } else if (text !== "") notesFiltersStore.getState().setText("");
                  else notesFiltersStore.getState().dropLastChip();
                } else if (
                  event.key === "Backspace" &&
                  event.currentTarget.selectionStart === 0 &&
                  event.currentTarget.selectionEnd === 0 &&
                  chips.length
                ) {
                  event.preventDefault();
                  notesFiltersStore.getState().removeTag(chips[chips.length - 1].tag);
                  setToken(null);
                } else if (["Tab", "ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key))
                  setToken(null);
              }}
            />
            <div
              ref={chipFlow}
              data-slot="filter-tags"
              onPointerUp={() => input.current?.focus()}
              className={cn(
                capped ? "flex w-full min-w-0 flex-wrap gap-1 overflow-y-auto" : "contents",
                capped && (phone ? "max-h-[140px]" : "max-h-20"),
              )}
            >
              {children}
            </div>
          </div>
          <IconHint label="Clear search">
            <Button
              type="button"
              variant="ghost"
              aria-label="Clear search"
              disabled={text === ""}
              className={cn(
                "ml-1 shrink-0 p-0 text-muted-foreground",
                phone ? "size-11" : "size-6",
              )}
              onClick={() => {
                setToken(null);
                notesFiltersStore.getState().setText("");
                input.current?.focus();
              }}
            >
              <X aria-hidden="true" className="size-4" />
            </Button>
          </IconHint>
          <span id={`${id}-description`} className="sr-only">
            {description}
          </span>
        </div>
      </PopoverAnchor>
      {open && (
        <TagSuggest
          id={id}
          matches={matches}
          active={active}
          chips={chips}
          phone={phone}
          loading={loading}
          error={error}
          onChoose={choose}
        />
      )}
    </Popover>
  );
}
