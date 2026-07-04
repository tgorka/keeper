import { Info } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

/**
 * Title naming the specific coverage gap (not a vague "some chats may be
 * unavailable"). Voice rules (UX-DR10): sentence case, no exclamation marks,
 * glossary noun Chat capitalized.
 */
export const DISCLOSURE_TITLE = "On-Device Chats won't appear in keeper";

/**
 * Leads with the literal required sentence naming the broken expectation
 * (FR-7). This exact string is asserted by tests and MUST NOT change.
 */
export const DISCLOSURE_WHATSAPP_SENTENCE =
  "WhatsApp connected in the official Beeper app will not appear here.";

/**
 * Explains why: On-Device Connections run inside the official Beeper app and
 * never reach the Beeper servers keeper syncs from.
 */
export const DISCLOSURE_EXPLANATION =
  "Beeper's On-Device Connections run inside the official Beeper app and never reach the " +
  "Beeper servers keeper syncs from, so keeper cannot see those Chats.";

/**
 * Names the self-hosted Bridge parity path (FR-7). Glossary noun Bridge
 * capitalized.
 */
export const DISCLOSURE_PARITY_SENTENCE = "Running your own Bridge is the path to parity.";

/**
 * The Beeper coverage disclosure (FR-7, UX-DR10). Presentational only: renders
 * the fixed, voice-rules-compliant copy (title + body) with no acknowledgment
 * button or Dialog chrome — callers supply the surrounding chrome (the login
 * gate's "I understand" button, or a settings Dialog with a Close). Usable both
 * inline and inside a Dialog.
 */
export function BeeperCoverageDisclosure() {
  return (
    <Alert>
      <Info aria-hidden="true" />
      <AlertTitle>{DISCLOSURE_TITLE}</AlertTitle>
      <AlertDescription>
        <p>{DISCLOSURE_WHATSAPP_SENTENCE}</p>
        <p>{DISCLOSURE_EXPLANATION}</p>
        <p>{DISCLOSURE_PARITY_SENTENCE}</p>
      </AlertDescription>
    </Alert>
  );
}
