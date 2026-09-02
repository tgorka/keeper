/**
 * One answer's body, rendered (Epic 61, Story 61.5).
 *
 * The parse lives in `src/lib/bots-markdown.ts`; this file is only the mapping
 * from its blocks to elements. Three properties are load-bearing enough that
 * changing this file without re-reading them will break something a test names:
 *
 * 1. **Nothing here can render HTML.** There is no `dangerouslySetInnerHTML`,
 *    no `innerHTML`, no `<img>`, no `<a href>`, no URL handed to anything that
 *    would fetch it. `note_protocol.rs` already stated the position — "a note
 *    that auto-fetches a URL an agent wrote is a tracking pixel" — and an
 *    answer is a longer string an agent wrote. A `<script>` the model emits is
 *    a paragraph of characters, and `bot-answer.test.tsx` asserts it.
 * 2. **A link is text, not a destination.** The label and, when it differs, the
 *    URL are both shown; neither is clickable. Making it clickable is a
 *    separate decision about opening an external browser from a model's output,
 *    and a link that silently goes nowhere on click would be the dead
 *    affordance AD-27 forbids. Shown-and-copyable is the honest middle.
 * 3. **The renderer is called on every token.** `BotBlock` is memoised on the
 *    block's own markdown, and keys are the parser's position-derived ones, so
 *    a delta that grows the last block re-renders the last block. Settled
 *    blocks keep their DOM nodes — asserted by identity, not by eye.
 *
 * `whitespace-pre-wrap` survives from the plain-text version, on prose and on
 * code alike: a model's line breaks inside a paragraph are information, and the
 * timeline has always shown them. `break-words` for the same reason it was
 * there before — a 300-character URL must not widen the column.
 */
import { memo, useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import {
  BOT_CODE_COPIED_LABEL,
  BOT_CODE_COPY_LABEL,
  BOT_CODE_PLAIN_LABEL,
  type MdBlock,
  type MdInline,
  parseAnswer,
} from "@/lib/bots-markdown";
import { cn } from "@/lib/utils";

/** How long the copy control stays confirmed. Long enough to read, short
 *  enough that a stale "Copied" never outlives the gesture. */
const COPIED_RESET_MS = 2000;

/** Heading sizes. An answer is body text with structure, not a document: the
 *  levels differ by weight and a little size, and none of them competes with
 *  the pane's own headings. `text-title` is the largest step an answer is
 *  allowed — a model that opens with `#` is not entitled to the display step,
 *  and DESIGN.md's six-step scale has no `text-base` in it. */
const HEADING_CLASS: Record<number, string> = {
  1: "font-semibold text-title",
  2: "font-semibold text-sm",
  3: "font-semibold text-sm",
  4: "font-medium text-sm",
  5: "font-medium text-sm",
  6: "font-medium text-muted-foreground text-sm",
};

/** The element each level becomes. A model's `##` is a real heading — a screen
 *  reader user navigating an answer by heading is the whole point — and the
 *  classes above are what keep it from competing with the pane's own. */
const HEADING_TAG: Record<number, "h1" | "h2" | "h3" | "h4" | "h5" | "h6"> = {
  1: "h1",
  2: "h2",
  3: "h3",
  4: "h4",
  5: "h5",
  6: "h6",
};

function InlineRuns({ runs }: { runs: MdInline[] }) {
  return (
    <>
      {runs.map((run, index) => {
        // Position-derived, like the block keys and for the same reason: runs
        // are re-created on every delta and only their order is stable.
        const key = index;
        switch (run.kind) {
          case "text":
            return <span key={key}>{run.text}</span>;
          case "code":
            return (
              <code key={key} className="rounded bg-muted px-1 py-0.5 font-mono text-[0.9em]">
                {run.text}
              </code>
            );
          case "emphasis":
            return (
              <em key={key}>
                <InlineRuns runs={run.children} />
              </em>
            );
          case "strong":
            return (
              <strong key={key} className="font-semibold">
                <InlineRuns runs={run.children} />
              </strong>
            );
          case "strike":
            return (
              <s key={key}>
                <InlineRuns runs={run.children} />
              </s>
            );
          case "link": {
            const label = run.children;
            const shown = label.length === 1 && label[0]?.kind === "text" ? label[0].text : null;
            return (
              <span key={key}>
                <InlineRuns runs={label} />
                {run.url !== "" && run.url !== shown && (
                  <span className="text-muted-foreground"> ({run.url})</span>
                )}
              </span>
            );
          }
        }
        // Unreachable, and typed so it stays that way: a `never` binding turns
        // "somebody added a run kind" into a compile error rather than a run
        // that silently renders nothing. The explicit return is what stops the
        // callback falling off its end into `undefined`.
        const unhandled: never = run;
        void unhandled;
        return null;
      })}
    </>
  );
}

function CodeBlock({
  language,
  text,
  closed,
}: {
  language: string | null;
  text: string;
  closed: boolean;
}) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) {
      return;
    }
    const timer = setTimeout(() => setCopied(false), COPIED_RESET_MS);
    return () => clearTimeout(timer);
  }, [copied]);
  return (
    <div
      className="overflow-hidden rounded-md border border-border"
      data-testid="bot-code-block"
      data-closed={closed ? "true" : "false"}
      data-language={language ?? ""}
    >
      <div className="flex items-center justify-between gap-2 border-border border-b bg-muted px-2 py-1">
        <span className="text-muted-foreground text-xs">{language ?? BOT_CODE_PLAIN_LABEL}</span>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-6 px-2 text-xs"
          onClick={() => {
            // Best effort, exactly as the recordings row and the recovery-key
            // card are: a clipboard the webview refuses is not worth a dialog,
            // and the code is on screen either way.
            void navigator.clipboard
              ?.writeText(text)
              .then(() => setCopied(true))
              .catch(() => {});
          }}
        >
          {copied ? BOT_CODE_COPIED_LABEL : BOT_CODE_COPY_LABEL}
        </Button>
      </div>
      <pre className="overflow-x-auto px-2 py-1.5">
        <code className="font-mono text-xs">{text}</code>
      </pre>
    </div>
  );
}

function BlockBody({ block }: { block: MdBlock }) {
  switch (block.kind) {
    case "paragraph":
      return (
        <p className="whitespace-pre-wrap break-words">
          <InlineRuns runs={block.children} />
        </p>
      );
    case "heading": {
      // A real `h1`-`h6`, so heading navigation works inside an answer. The
      // level is data, so the tag comes out of a table rather than out of six
      // near-identical branches.
      const Tag = HEADING_TAG[block.level] ?? "h6";
      return (
        <Tag className={cn("break-words", HEADING_CLASS[block.level])}>
          <InlineRuns runs={block.children} />
        </Tag>
      );
    }
    case "code":
      return <CodeBlock language={block.language} text={block.text} closed={block.closed} />;
    case "list": {
      const className = cn(
        "flex flex-col gap-1 break-words pl-5",
        block.ordered ? "list-decimal" : "list-disc",
      );
      const items = block.items.map((item) => (
        <li key={item.key}>
          <BlockList blocks={item.blocks} />
        </li>
      ));
      return block.ordered ? (
        <ol className={className} start={block.start}>
          {items}
        </ol>
      ) : (
        <ul className={className}>{items}</ul>
      );
    }
    case "quote":
      return (
        <blockquote className="border-border border-l-2 pl-3 text-muted-foreground">
          <BlockList blocks={block.blocks} />
        </blockquote>
      );
    case "rule":
      return <hr className="border-border" />;
    case "table":
      return (
        <div className="overflow-x-auto">
          <table className="w-full border-collapse text-left">
            {block.header.length > 0 && (
              <thead>
                <tr>
                  {block.header.map((cell, index) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: a cell has no identity but its column, and position keys are what keep a streamed table from remounting per token — see the parser's module note
                    <th key={index} className="border-border border-b px-2 py-1 font-medium">
                      <InlineRuns runs={cell} />
                    </th>
                  ))}
                </tr>
              </thead>
            )}
            <tbody>
              {block.rows.map((row, rowIndex) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: reason above — a row's identity is its position in the table
                <tr key={rowIndex}>
                  {row.map((cell, cellIndex) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: reason above.
                    <td key={cellIndex} className="border-border border-b px-2 py-1">
                      <InlineRuns runs={cell} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    case "literal":
      // Outside the subset — an HTML block, a link reference definition, a
      // construct a later parser version invents. The characters, shown.
      return <p className="whitespace-pre-wrap break-words font-mono text-xs">{block.source}</p>;
  }
}

/**
 * One block, memoised on its own markdown.
 *
 * The comparison is `source` plus `kind` rather than a deep compare, because
 * the parser rebuilds every object on every delta: reference equality would
 * never hold and a deep compare would cost more than the render it saves. Two
 * blocks with the same key, kind and source render identically by construction
 * — the block IS a function of its source.
 */
const BotBlock = memo(
  ({ block }: { block: MdBlock }) => <BlockBody block={block} />,
  (previous, next) =>
    previous.block.kind === next.block.kind && previous.block.source === next.block.source,
);
BotBlock.displayName = "BotBlock";

function BlockList({ blocks }: { blocks: MdBlock[] }) {
  return (
    <div className="flex min-w-0 flex-col gap-2">
      {blocks.map((block) => (
        <BotBlock key={block.key} block={block} />
      ))}
    </div>
  );
}

export function BotAnswer({ body }: { body: string }) {
  // Deliberately not `useMemo`: the parse is a function of `body`, and `body`
  // changes on every delta, so a memo would only add a cache that never hits.
  // The saving that matters is per-block, and it is the memo above.
  return (
    <div className="min-w-0 text-sm" data-testid="bot-answer">
      <BlockList blocks={parseAnswer(body)} />
    </div>
  );
}
