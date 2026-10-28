/**
 * A message bubble's tokens (FR-8/FR-9, UX-DR5), shared by the chat timeline and an agent
 * session's own lines (R-HQ-86): one shape, and one surface for what the person wrote.
 */

/** A 14 px radius, at most 75% of its column. */
export const BUBBLE_SHAPE = "max-w-[75%] rounded-[14px] px-3 py-2 text-sm";

/** The person's own message: the primary surface; its row aligns it right. */
export const OWN_BUBBLE = "bg-primary text-primary-foreground";
