# Epic 4 Context: Unified Inbox Organization

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Turn the already-merged multi-account inbox (built in Epic 2) into a Beeper-grade triage surface. This epic adds the organizational layer that makes a large, cross-account, cross-network inbox usable: accurate unread and mention state with manual read/unread control, an Archive view that auto-returns chats on new activity, a Pins strip and always-visible Favorites section, per-Matrix-Space filtering, and unambiguous per-row Network and Account attribution with a simple Network filter. It completes the Unified Inbox surface (FR-18) so that identical remote contacts reached via different accounts or networks are never confusable, and inbox-zero triage is a safe, reversible flow.

## Stories

- Story 4.1: Unread Management
- Story 4.2: Archive View with Auto-Return
- Story 4.3: Pins
- Story 4.4: Favorites
- Story 4.5: Spaces as Room-Group Views
- Story 4.6: Network & Account Attribution and Network Filter

## Requirements & Constraints

- Unread state per chat: filled primary badge for mentions, neutral dot otherwise, chat name at weight 600 — bold means "unread" and nothing else, never used for any other emphasis. Must match server-side read-marker state after sync convergence. Manual "Mark read" / "Mark unread" updates locally within one frame and round-trips to the server read marker.
- Archiving removes a chat from the inbox into an Archive view; unarchiving returns it to its correct chronological position. An archived chat automatically returns to the inbox on new activity. Archive state must persist across relaunch and sync across the user's other Matrix clients where representable (low-priority tag semantics).
- Pins render as a horizontal strip of circular avatars at the top of the chat list, removed from the chronological flow below. Pin order is user-controlled by drag and must persist across restarts. Pinned chats stay pinned regardless of newer activity elsewhere; overflow beyond 8 pins scrolls horizontally.
- Favorites is an always-visible labeled section between the Pins strip and the inbox scroll — reachable in one interaction from any scroll position. Favorite state and the section's collapse/expand state persist across restart and re-login (server-side tag where representable). When empty, the section is hidden entirely.
- Spaces are view-and-filter only: list each Matrix Space the user belongs to and filter the inbox to that Space's rooms. No create/edit/join/leave/hierarchy management anywhere. Space membership changes from sync must update the list and filter results.
- Every chat row and chat header must carry a Network badge and an Account marker so two chats with the same remote contact via different accounts always differ visibly. One Network filter and one Space filter may compose (AND); the active combination renders as dismissible chips above the chat list; clearing a filter (chip or Esc from the list) restores the inbox.
- Performance floor: 60 fps inbox scroll at 10k chats; the UI must never ship all rows to JS or re-derive ordering/filtering client-side.

## Technical Decisions

- The Unified Inbox is computed entirely in `keeper-core::inbox` (AD-20): it merges N per-account `RoomListService` streams, orders by recency, hoists Pins, sections Favorites, applies Archive state, and applies Space/Network filters. The UI receives a **windowed** view-model stream (visible range + buffer, with totals). Ordering, filtering, unread counts, and sectioning are never derived or re-sorted in TypeScript — the frontend only applies diff batches.
- Unread counts and all inbox-organizational state are computed in Rust and streamed; the TS layer renders authoritative view models only, never inventing state.
- Follow the established IPC contract (AD-8): `domain_verb` snake_case commands, one channel per subscription, snapshot batch always delivered before any diff batch, `keeper://kebab-case` events. Mutating actions (archive/unarchive, pin/unpin, reorder, favorite, mark read/unread) are commands that then reflect back through the inbox stream.
- Frontend state lives in the single `inbox` zustand vanilla mirror store (AD-9), applying diffs imperatively; components subscribe via selectors. No TanStack Query or component-local reducers for this server-originated state. Stores in `src/stores/` named `use<Domain>Store`.
- Persistence and cross-client sync of Archive/Favorite/Pin state ride Matrix tag semantics (favourite tag, low-priority tag for archive) where representable; where a state has no server representation, it persists locally.
- The `inbox` module owns FR-18–24; new logic defaults into `keeper-core`, with only IPC glue in the Tauri `keeper` crate.

## UX & Interaction Patterns

- **Chat row (64 px):** avatar with a 16 px Network badge overlaid bottom-right (2 px background ring), name + timestamp line, preview line, right-aligned unread badge / draft marker / mute-bell glyph. Account attribution is a 3 px left-edge bar in the account's assigned hue (8-hue wheel, assigned at add time). Network identity appears **only** as the badge — never as per-network coloring of rows, panes, or bubbles.
- **Pins strip:** circular 44 px avatars, no labels, network badge overlaid, drag to reorder.
- **Favorites section:** labeled `section-label` group (uppercase "FAVORITES") of compact 48 px rows.
- **Sidebar structure:** primary views (Inbox ⌘1, Archive ⌘2) → SPACES group (per-Space rows) → NETWORKS group (filter chips per connected Network, each with a bridge-health dot). Space and Network chips are single-select per group and compose as AND; the active filter shows as a dismissible chip above the chat list, and Esc from the list clears the filter before moving focus.
- **Context menu on a chat row:** Archive, Mark read/unread, Pin, Favorite (single-key equivalents `e`/`u`/`p`/`f` arrive in Epic 9 — not in this epic).
- **Empty states (voice: sentence case, no exclamation marks, Glossary-capitalized nouns like Chat/Account/Network):** Archive — "Nothing archived. `E` archives a chat and keeps it searchable." Empty filtered inbox — "No chats in {filter}." with a Clear filter action. Favorites empty — section hidden, with a one-time hint surfaced in the chat-row context menu.
- Accessibility floor applies: keyboard-operable, VoiceOver labels reflecting dynamic state, visible focus rings, focus return on overlay close.

## Cross-Story Dependencies

- The whole epic depends on **Epic 2** for the Rust-side multi-account inbox merge and the windowed inbox projection; Epic 4 organizes that stream rather than building it.
- Stories 4.2 (Archive), 4.3 (Pins), 4.4 (Favorites), 4.5 (Spaces) each depend on Story 4.1 (unread management establishes the inbox-row + inbox-projection baseline they extend).
- Story 4.6 (attribution + Network filter) additionally depends on Story 4.5 for filter composition (Network filter AND Space filter share the same dismissible-chip mechanism).
- Single-key list verbs (`e`/`u`/`p`/`f`/`m`) referenced by these interactions are deferred to Epic 9; Epic 4 wires the same actions via context menu.
- FR-24 attribution reuses the 8-hue account marker and network-badge conventions already established in Epic 2.
