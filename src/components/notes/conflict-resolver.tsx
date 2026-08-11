/**
 * Conflict mode (Story 38.6, FR-116, NFR-30).
 *
 * A conflict is two texts that both exist and both matter, so the surface shows
 * both and asks the user which parts to keep — in the editor pane, never in a
 * modal, with no deadline and no auto-resolution. Escape abandons and writes
 * nothing; both files stay exactly where they were.
 *
 * `Finish` sends the assembled body through `notes_resolve_conflict`, which
 * writes the resolution and deletes the conflict copy **in one commit**, so the
 * commit that removes the copy is the commit whose parent still contains it.
 * That ordering is the whole of NFR-30 in this path.
 *
 * The block alignment below is paragraph-level on purpose. Line-level would
 * produce a shredded review of two prose documents; paragraph-level produces
 * the units a person actually chooses between.
 */
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { notesResolveConflict } from "@/lib/ipc/client";

/** Above this, the pairwise alignment is not worth its quadratic cost and the
 *  whole document is offered as one choice. */
const MAX_BLOCKS = 400;

export type ConflictBlock =
  | { kind: "same"; text: string }
  | { kind: "differs"; mine: string; theirs: string };

/** Which side of a differing block the user kept. */
export type ConflictChoice = "mine" | "theirs" | "both";

/** Paragraphs, with their blank-line separators dropped and re-added on join. */
function splitBlocks(text: string): string[] {
  return text.split(/\n\s*\n/).filter((block) => block.trim() !== "");
}

/**
 * Align two versions into shared blocks and differing pairs.
 *
 * A longest-common-subsequence walk over paragraphs, so an insertion in the
 * middle of one side does not push everything after it into the diff.
 */
export function alignBlocks(mineText: string, theirsText: string): ConflictBlock[] {
  const mine = splitBlocks(mineText);
  const theirs = splitBlocks(theirsText);
  if (mine.length > MAX_BLOCKS || theirs.length > MAX_BLOCKS) {
    return [{ kind: "differs", mine: mineText, theirs: theirsText }];
  }

  // Suffix LCS lengths: table[i][j] is the LCS of mine[i..] and theirs[j..].
  const table: number[][] = Array.from({ length: mine.length + 1 }, () =>
    new Array<number>(theirs.length + 1).fill(0),
  );
  for (let i = mine.length - 1; i >= 0; i -= 1) {
    for (let j = theirs.length - 1; j >= 0; j -= 1) {
      table[i][j] =
        mine[i] === theirs[j]
          ? table[i + 1][j + 1] + 1
          : Math.max(table[i + 1][j], table[i][j + 1]);
    }
  }

  const blocks: ConflictBlock[] = [];
  let heldMine: string[] = [];
  let heldTheirs: string[] = [];
  const flush = (): void => {
    if (heldMine.length > 0 || heldTheirs.length > 0) {
      blocks.push({
        kind: "differs",
        mine: heldMine.join("\n\n"),
        theirs: heldTheirs.join("\n\n"),
      });
      heldMine = [];
      heldTheirs = [];
    }
  };

  let i = 0;
  let j = 0;
  while (i < mine.length || j < theirs.length) {
    if (i < mine.length && j < theirs.length && mine[i] === theirs[j]) {
      flush();
      blocks.push({ kind: "same", text: mine[i] });
      i += 1;
      j += 1;
    } else if (j < theirs.length && (i === mine.length || table[i][j + 1] >= table[i + 1][j])) {
      heldTheirs.push(theirs[j]);
      j += 1;
    } else {
      heldMine.push(mine[i]);
      i += 1;
    }
  }
  flush();
  return blocks;
}

/**
 * The body the chosen sides add up to.
 *
 * An unresolved block contributes nothing, which is why `Finish` stays disabled
 * until every one of them has been answered — an unanswered block must never
 * become a silently dropped paragraph.
 */
export function assemble(
  blocks: readonly ConflictBlock[],
  choices: readonly (ConflictChoice | null)[],
): string {
  const parts: string[] = [];
  let differing = 0;
  for (const block of blocks) {
    if (block.kind === "same") {
      parts.push(block.text);
      continue;
    }
    const choice = choices[differing];
    differing += 1;
    if (choice === "mine" && block.mine !== "") {
      parts.push(block.mine);
    } else if (choice === "theirs" && block.theirs !== "") {
      parts.push(block.theirs);
    } else if (choice === "both") {
      parts.push([block.mine, block.theirs].filter((side) => side !== "").join("\n\n"));
    }
  }
  return `${parts.join("\n\n")}\n`;
}

export interface ConflictResolverProps {
  vaultId: string;
  noteId: string;
  /** This machine's version — the editor's buffer. */
  mine: string;
  /** The other side, as delivered by the body channel or the conflict copy. */
  theirs: string;
  /** Label for the other side, e.g. the device that wrote it. */
  theirsLabel?: string;
  /** Resolution succeeded; the caller returns to the editor. */
  onResolved: () => void;
  /** Escape: nothing is written and the conflict stays. */
  onAbandon: () => void;
}

export function ConflictResolver({
  vaultId,
  noteId,
  mine,
  theirs,
  theirsLabel = "the other device",
  onResolved,
  onAbandon,
}: ConflictResolverProps) {
  const blocks = alignBlocks(mine, theirs);
  const differingCount = blocks.filter((block) => block.kind === "differs").length;
  const [choices, setChoices] = useState<(ConflictChoice | null)[]>(() =>
    new Array<ConflictChoice | null>(differingCount).fill(null),
  );
  const [focused, setFocused] = useState(0);
  const [failure, setFailure] = useState<string | null>(null);

  const resolved = choices.filter((choice) => choice !== null).length;
  const choose = (index: number, choice: ConflictChoice): void => {
    setChoices((previous) => previous.map((held, at) => (at === index ? choice : held)));
  };

  const finish = (): void => {
    void notesResolveConflict(vaultId, noteId, {
      kind: "merged",
      text: assemble(blocks, choices),
    })
      .then(onResolved)
      .catch(() => {
        setFailure("keeper couldn't write the resolution. Both versions are still on disk.");
      });
  };

  let differingIndex = -1;
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the side-choosing
    // grammar (⌘Enter / ⌘⌫ / b / ⌥⌘↕) is a surface-wide keyboard affordance;
    // every block keeps its own labelled Keep buttons, so this is additive.
    <section
      aria-label="Resolve conflict"
      className="flex h-full flex-col"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          onAbandon();
        } else if (event.key === "Enter" && event.metaKey) {
          event.preventDefault();
          choose(focused, "mine");
        } else if (event.key === "Backspace" && event.metaKey) {
          event.preventDefault();
          choose(focused, "theirs");
        } else if (event.key === "b" && !event.metaKey && !event.ctrlKey) {
          event.preventDefault();
          choose(focused, "both");
        } else if (event.key === "ArrowDown" && event.metaKey && event.altKey) {
          event.preventDefault();
          setFocused((at) => Math.min(at + 1, differingCount - 1));
        } else if (event.key === "ArrowUp" && event.metaKey && event.altKey) {
          event.preventDefault();
          setFocused((at) => Math.max(at - 1, 0));
        }
      }}
    >
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-2 text-sm">
        {blocks.map((block) => {
          if (block.kind === "same") {
            return (
              <p key={block.text} className="whitespace-pre-wrap py-1">
                {block.text}
              </p>
            );
          }
          differingIndex += 1;
          const index = differingIndex;
          const choice = choices[index];
          return (
            <div
              key={`differs-${index}`}
              className={`my-2 rounded border ${index === focused ? "ring-1 ring-ring" : ""}`}
            >
              <ConflictSide
                label="This Mac"
                text={block.mine}
                active={choice === "mine" || choice === "both"}
                onKeep={() => {
                  setFocused(index);
                  choose(index, "mine");
                }}
              />
              <ConflictSide
                label={theirsLabel}
                text={block.theirs}
                active={choice === "theirs" || choice === "both"}
                onKeep={() => {
                  setFocused(index);
                  choose(index, "theirs");
                }}
              />
              <div className="border-t px-2 py-1">
                <Button size="sm" variant="ghost" onClick={() => choose(index, "both")}>
                  Keep both
                </Button>
              </div>
            </div>
          );
        })}
      </div>
      <div className="flex items-center gap-2 border-t px-3 py-1.5 text-xs">
        <span className="flex-1">
          {resolved} of {differingCount} resolved
        </span>
        {failure === null ? null : <span className="text-destructive">{failure}</span>}
        <Button size="sm" disabled={resolved < differingCount} onClick={finish}>
          Finish
        </Button>
      </div>
    </section>
  );
}

interface ConflictSideProps {
  label: string;
  text: string;
  active: boolean;
  onKeep: () => void;
}

function ConflictSide({ label, text, active, onKeep }: ConflictSideProps) {
  return (
    <div className={`border-b px-2 py-1 last:border-b-0 ${active ? "bg-muted" : ""}`}>
      <div className="flex items-center gap-2">
        {/* `text-muted-foreground`, not `text-faint`: this word is the only
            thing telling the two otherwise identical blocks apart. */}
        <span className="flex-1 label-caps text-muted-foreground">{label}</span>
        <Button size="sm" variant="ghost" onClick={onKeep}>
          Keep
        </Button>
      </div>
      <p className="whitespace-pre-wrap text-sm">{text === "" ? "(nothing here)" : text}</p>
    </div>
  );
}
