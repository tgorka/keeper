import { BUBBLE_SHAPE, OWN_BUBBLE } from "@/components/chat/bubble";
import { cn } from "@/lib/utils";

/** The row holding the person's own line: its name, bubble and files stack at the right edge. */
export const MINE_ROW = "flex flex-col items-end";

/**
 * A person's line in an agent session (R-HQ-86). This device's person's own — Rust's `mine`, from
 * any of their devices — is a bubble on the primary surface, as the chat timeline draws one's own
 * message, so a question does not get lost among the answers. Anyone else's line stays plain text
 * on the left: a session can hold several people, and only one of them is "me".
 */
export function PersonLine({ body, mine }: { body: string; mine: boolean }) {
  return (
    <p
      data-mine={mine ? "" : undefined}
      className={cn(
        "whitespace-pre-wrap break-words text-sm [overflow-wrap:anywhere]",
        mine && [BUBBLE_SHAPE, OWN_BUBBLE],
      )}
    >
      {body}
    </p>
  );
}
