/**
 * Agents at launch, on the default agent (story 100.3, R383, R397).
 *
 * Once per app session, after the accounts are hydrated, ask Rust what to open
 * (`defaultAgentStore.launch`) and apply the answer exactly once. Rust answers
 * when it knows — once every signed-in account has finished its first sync, or
 * as soon as the default agent's room is here, never later than its bound —
 * so nothing here waits for the Agents window or reads its rooms: the Agents window, and the default agent's
 * `main` room opened (and, on the desktop, its composer focused) when Rust
 * named one. Which room — or none, and why — is Rust's
 * (`keeper_core::agents::launch::choose`); this hook decides nothing about it.
 *
 * The person always wins: any change of window or room made before the
 * answer lands (a click, a notification, a deep link) cancels the launch, and
 * nothing is ever applied a second time — not after an account-set
 * re-subscribe, not when the person later returns to the Inbox.
 *
 * On the phone the rule applies only when the restored navigation (Story
 * 14.4) had nothing to restore: a webview reload after a jettison lands where
 * the person was, never on the default agent.
 *
 * When Rust says its reason is to be announced (`AgentsLaunchVm.announce`:
 * the Agents window opens with nothing selected because the default agent
 * has no room here, or the launch stopped waiting before it knew — the Inbox
 * included), the reason is a notice where the person is looking (and a WARN
 * line Rust already wrote); a launch Rust could not answer leaves the Inbox
 * open, says so, and reports the error to `keeper.log`. Opening the Agents
 * window is counted once as the closed `launchDefaultAgent` surface (R391),
 * with nothing about the agent or the room.
 */
import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { captureSurface } from "@/hooks/use-telemetry";
import { reportFrontendError } from "@/lib/crash-report";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useCapabilitiesStore, useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { composerStore } from "@/lib/stores/composer";
import { defaultAgentStore, sentenceOf, useDefaultAgentStore } from "@/lib/stores/default-agent";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

/** The notice when Rust could not answer what to open. */
export function launchFailedSentence(error: unknown): string {
  return `keeper could not open your default agent, so the Inbox is open: ${sentenceOf(error)}`;
}

/** How long the launch's reason stays on screen. */
export const REASON_NOTICE_MS = 12_000;

export function useLaunchIntoAgents(): void {
  const hydrated = useAccountsStore((s) => s.hydrated);
  const hasAccount = useAccountsStore((s) => s.accounts.length > 0);
  const capabilitiesHydrated = useCapabilitiesStore((s) => s.hydrated);
  const phone = useIsReducedCapabilityPlatform();
  const navRestore = useDefaultAgentStore((s) => s.navRestore);
  // Asked once per app session; the answer is applied at most once.
  const askedRef = useRef(false);
  // The person moved (window or room) since the app opened.
  const navigatedRef = useRef(false);
  // The launch itself is moving the stores: not the person's navigation.
  const applyingRef = useRef(false);

  useEffect(() => {
    const offView = primaryViewStore.subscribe((state, prev) => {
      if (state.view !== prev.view && !applyingRef.current) {
        navigatedRef.current = true;
      }
    });
    const offRoom = roomsStore.subscribe((state, prev) => {
      if (state.selected !== prev.selected && !applyingRef.current) {
        navigatedRef.current = true;
      }
    });
    return () => {
      offView();
      offRoom();
    };
  }, []);

  useEffect(() => {
    if (
      askedRef.current ||
      !hydrated ||
      !hasAccount ||
      !capabilitiesHydrated ||
      (phone && navRestore === "pending")
    ) {
      return;
    }
    askedRef.current = true;
    if (navigatedRef.current || (phone && navRestore === "restored")) {
      return;
    }
    void defaultAgentStore
      .getState()
      .launch()
      .then(
        (vm) => {
          if (navigatedRef.current) {
            return;
          }
          // Rust decides whether its reason is said at launch: a finished
          // look that found no agent room is the Inbox as before, quietly; a
          // launch that stopped waiting says so on whichever window it left.
          const announce = () => {
            if (vm.announce && vm.reason !== null) {
              // Long enough to read: it explains what the person is looking at.
              toast(vm.reason, { duration: REASON_NOTICE_MS });
            }
          };
          if (vm.view !== "agents") {
            announce();
            return;
          }
          void captureSurface("launchDefaultAgent");
          applyingRef.current = true;
          primaryViewStore.getState().setView("agents");
          if (vm.room !== null) {
            roomsStore.getState().selectRoom(vm.room);
            // Desktop keeps focus-on-open; the phone never steals composer
            // focus when a room opens (UX-DR22). After the room's composer has
            // mounted: a fresh composer seeds its focus nonce at mount, so a
            // bump made in the same tick as the selection is never seen.
            if (!phone) {
              window.setTimeout(() => {
                const selected = roomsStore.getState().selected;
                if (selected?.roomId === vm.room?.roomId) {
                  composerStore.getState().requestFocus();
                }
              }, 0);
            }
          }
          applyingRef.current = false;
          announce();
        },
        (error: unknown) => {
          reportFrontendError("agents.launch", error, null);
          toast.error(launchFailedSentence(error));
        },
      );
  }, [hydrated, hasAccount, capabilitiesHydrated, phone, navRestore]);
}
