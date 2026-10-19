/**
 * Agents at launch, on the default agent (story 100.3, R383, R397).
 *
 * Once per app session, after the accounts are hydrated, ask Rust what to open
 * (`defaultAgentStore.launch`) and apply the answer exactly once. Rust answers
 * when it knows — once every signed-in account has finished its first sync, or
 * as soon as the default agent's room is here, never later than its bound —
 * so nothing here waits for the Agents window or reads its rooms: the Agents
 * window, and the default agent's `main` room opened with its composer
 * focused — on the desktop and the phone (R463) — when Rust named one. Which
 * room — or none, and why — is Rust's
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
 * When the default agent is one of the person's own agents with no
 * conversation here (`AgentsLaunchVm.ask`, R416), Rust is asked to open one
 * (`openMainDm`): "Opening a conversation with Nixi…" while it waits, the
 * room selected once it lands — if the person has not moved meanwhile — or
 * Rust's sentence for why not.
 *
 * When Rust says its reason is to be announced (`AgentsLaunchVm.announce`:
 * the Agents window opens with nothing selected because the default agent
 * has no room here, or the launch stopped waiting before it knew — the Inbox
 * included), the reason is a notice where the person is looking (and a WARN
 * line Rust already wrote); a launch Rust could not answer leaves the Inbox
 * open, says so, and reports the error to `keeper.log`. Opening the Agents
 * window is counted once as the closed `launchDefaultAgent` surface (R391),
 * with nothing about the agent or the room.
 *
 * With no default chosen, Rust may say to ask which agent opens
 * (`AgentsLaunchVm.chooser`, R463): the Agents window opens and its chooser
 * is shown once (`defaultAgentStore.openChooser`), non-blocking — the
 * person may ignore it, choose, or put it away.
 */
import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { captureSurface } from "@/hooks/use-telemetry";
import { reportFrontendError } from "@/lib/crash-report";
import { openAndFocus } from "@/lib/default-agent-actions";
import type { LaunchRoomVm } from "@/lib/ipc/client";
import { openMainDm } from "@/lib/main-dm";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useCapabilitiesStore, useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
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
          // The room opens with its composer focused, on the phone too
          // (R463): the phone shows the conversation, not the list.
          const open = (room: LaunchRoomVm) => {
            applyingRef.current = true;
            openAndFocus(room);
            applyingRef.current = false;
          };
          applyingRef.current = true;
          primaryViewStore.getState().setView("agents");
          applyingRef.current = false;
          if (vm.room !== null) {
            open(vm.room);
          }
          announce();
          // Nothing chosen yet: ask, once, which agent opens (R463).
          if (vm.chooser) {
            defaultAgentStore.getState().openChooser();
          }
          // The default agent has no conversation here: Rust asks it for one
          // and the room opens when it lands — unless the person has moved
          // on meanwhile (story 101.1 #4, R416).
          if (vm.ask !== null) {
            void openMainDm(vm.ask).then((room) => {
              if (room !== null && !navigatedRef.current) {
                open(room);
              }
            });
          }
        },
        (error: unknown) => {
          reportFrontendError("agents.launch", error, null);
          toast.error(launchFailedSentence(error));
        },
      );
  }, [hydrated, hasAccount, capabilitiesHydrated, phone, navRestore]);
}
