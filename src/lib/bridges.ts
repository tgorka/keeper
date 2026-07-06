/**
 * Shared Bridges-surface constants and presentation helpers (Story 6.2, FR-25,
 * UX-DR13).
 *
 * The single source of truth for the companion-stack docs link and the discovery
 * status → label map, so the pane and card render identical copy without either
 * inventing status text. Risk-tier badges and ack copy still come only from the
 * backend catalog data — nothing here duplicates that.
 */
import type { BbctlPhase, BridgeHealth, BridgeLoginPhase, BridgeStatus } from "@/lib/ipc/client";

/**
 * The companion-stack docs link surfaced in the "No bridges found" empty state
 * (UX-DR13). No hosted companion stack exists yet — this points at keeper's real
 * repository docs and is the single constant to repoint when a dedicated
 * companion-stack page lands. Never a fabricated hosted service.
 */
export const COMPANION_STACK_DOCS_URL = "https://github.com/tgorka/keeper/tree/main/docs";

/**
 * The short status word shown on a bridge card for each discovered
 * {@link BridgeStatus}. Setup/login state only — live connection health
 * (Connected / Disconnected in the 6.5 sense) is a separate placeholder dot.
 */
export const BRIDGE_STATUS_LABEL: Record<BridgeStatus, string> = {
  loggedIn: "Connected",
  notLoggedIn: "Action needed",
  configured: "Not set up",
};

/**
 * The live state word shown in the bridge login Sheet for each
 * {@link BridgeLoginPhase} (Story 6.3, FR-26). One transport-agnostic word per
 * phase so the stepper reads identically whichever transport powered the login.
 */
export const BRIDGE_LOGIN_PHASE_LABEL: Record<BridgeLoginPhase, string> = {
  choosingMethod: "Connecting",
  waiting: "Waiting",
  qr: "Scan QR",
  codeEntry: "Enter code",
  success: "Linked ✓",
  failure: "Couldn't connect",
};

/**
 * The log-free step word shown in the "Run your own bridge" run Sheet for each
 * {@link BbctlPhase} (Story 6.7, FR-29). Recognized phase transitions only — no raw
 * `bbctl` log line ever reaches the UI (there is no log viewer, v1.x).
 */
export const BBCTL_PHASE_LABEL: Record<BbctlPhase, string> = {
  checking: "Checking for bbctl",
  registering: "Registering bridge",
  starting: "Starting bridge",
  running: "Bringing it up",
  success: "Running ✓",
  failure: "Couldn't start",
};

/**
 * The live-health state word shown on a bridge card for each {@link BridgeHealth}
 * (Story 6.5, FR-28). Live connection health — distinct from the setup/login
 * {@link BRIDGE_STATUS_LABEL}. Rust owns the state; this only names it.
 */
export const BRIDGE_HEALTH_LABEL: Record<BridgeHealth, string> = {
  healthy: "Connected",
  degraded: "Action needed",
  disconnected: "Disconnected",
};

/** The `--bridge-*` dot tint for each live {@link BridgeHealth} (Story 6.5). */
export const BRIDGE_HEALTH_DOT_CLASS: Record<BridgeHealth, string> = {
  healthy: "bg-bridge-healthy",
  degraded: "bg-bridge-degraded",
  disconnected: "bg-bridge-disconnected",
};
