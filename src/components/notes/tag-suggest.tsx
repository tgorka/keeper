import { SignedPopover, type SignedPopoverProps } from "@/components/notes/signed-popover";

/** Caret entrance retains active-descendant focus in the search field. */
export function TagSuggest(props: SignedPopoverProps & { id: string; active: number }) {
  return <SignedPopover {...props} />;
}
