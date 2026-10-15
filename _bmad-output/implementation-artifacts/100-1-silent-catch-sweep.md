# Story 100.1 #6 — the front silent-catch sweep (rung `agents-100-errors`, 2026-10-14)

Every promise `.catch` that did nothing and every awaiting `try` with an empty `catch` in `src/` (outside `components/ui/`), as `src/test/no-silent-catch.test.ts` found them on base 269d6b93, and what each became. Lines are the base's; paths are relative to `src/`. **said** = `sayFailure(source, fallback)`: a toast in Rust's words or the fallback, and a `keeper.log` line. **logged** = `logFailure(source)`: the `keeper.log` line only (nobody can feel it, or another surface already shows it). The source name is the `source` field of the log line.

## Sites (245)

- App.tsx:55 — logged — app.studyStop — automatic stale-disclosure retry on boot; keeps the original "never log an arbitrary IPC error" rule by logging a fixed sentence, not the raw error
- components/account/setup-code-scanner.tsx:228 — said — setup.openCameraSettings — "Open Camera settings" click
- components/approval/approval-pane.tsx:167 — logged — approval.clearDraft — clean-up after a send that worked; the re-query shows any leftover
- components/approval/approval-pane.tsx:168 — logged — approval.clearDraftMirror — clean-up of the cross-device mirror after the send
- components/approval/approval-pane.tsx:184 — said — approval.discard — ⌘⌫ discard did not take
- components/approval/approval-pane.tsx:185 — logged — approval.clearDraftMirror — mirror tombstone, secondary to the local discard
- components/approval/approval-pane.tsx:194 — said — approval.undoDiscard — Undo click did not restore the draft
- components/approval/approval-pane.tsx:196 — logged — approval.mirrorDraft — mirror echo of the restore
- components/approval/approval-pane.tsx:217 — said — approval.saveEdit — inline edit save did not persist
- components/approval/approval-pane.tsx:218 — logged — approval.mirrorDraft — mirror echo of the edit
- components/auth/login-screen.tsx:76 — said — login.openLink — link click; docstring updated
- components/auth/login-screen.tsx:291 — logged — login.cancelOidc — cancel on unmount, the surface is gone
- components/auth/login-screen.tsx:373 — said — login.cancelOidc — Cancel click; otherwise the flow sits pending until timeout
- components/auth/login-screen.tsx:529 — logged — login.cancelBeeper — cancel on unmount
- components/bots/bot-answer.tsx:169 — said — bots.copyCode — Copy click
- components/bots/bot-attachment.tsx:328 — logged — bots.discardImage — staging clean-up; the chip is already gone
- components/bots/bot-attachment.tsx:428 — said — bots.revealDeliverable — reveal click
- components/bots/bot-message-meta.tsx:270 — logged — bots.metaShownGet — hydrate on mount; off is the honest default
- components/bots/bot-pins-strip.tsx:163 — said — bots.reorder — drag / Alt+arrow reorder did not persist
- components/bots/bot-pins-strip.tsx:378 — logged — bots.relist — background re-read after an identity save
- components/bots/bot-pins-strip.tsx:408 — said — bots.unpin — Unpin confirm
- components/bots/bot-voice-mic.tsx:147 — said — voice.stop — Stop / Cancel this question press
- components/bots/bot-voice-mic.tsx:217 — said — voice.openSettings — Open Settings click
- components/bots/bot-voice-target.tsx:109 — logged — voice.agentTargets — background listing, polled again
- components/bots/bot-voice-target.tsx:136 — logged — voice.targetSpeeds — the names still work without the numbers
- components/bots/bots-phone-pane.tsx:259 — said — voice.openSettings — Open Settings click
- components/bots/bots-phone-pane.tsx:473 — logged — bots.firstModel — automatic model pick; the sheet's picker shows the failure
- components/capture/capture-window.tsx:237 — logged — capture.listenWindows — listener attach; the buttons still work
- components/chat/chat-row.tsx:145 — said — chat.archive — context menu / swipe Archive
- components/chat/chat-row.tsx:148 — said — chat.unarchive — Unarchive
- components/chat/chat-row.tsx:154 — said — chat.pin — Pin
- components/chat/chat-row.tsx:157 — said — chat.unpin — Unpin
- components/chat/chat-row.tsx:163 — said — chat.favorite — Favorite
- components/chat/chat-row.tsx:166 — said — chat.unfavorite — Unfavorite
- components/chat/chat-row.tsx:173 — said — chat.notifyMode — Notifications radio
- components/chat/composer.tsx:287 — logged — composer.mirrorDraft — debounced mirror on typing
- components/chat/composer.tsx:289 — logged — composer.clearDraftMirror — debounced mirror tombstone
- components/chat/composer.tsx:300 — logged — composer.clearDraftMirror — mirror tombstone after send/clear
- components/chat/composer.tsx:332 — logged — composer.saveDraft — autosave on every keystroke (no toast spam)
- components/chat/composer.tsx:334 — logged — composer.clearDraft — autosave delete of an emptied draft
- components/chat/composer.tsx:352 — logged — composer.clearDraft — clean-up after send
- components/chat/composer.tsx:487 — logged — composer.loadDraft — restore on open, falls back to null (handler logs then returns null)
- components/chat/composer.tsx:488 — logged — composer.loadRemoteDraft — same, for the remote mirror
- components/chat/composer.tsx:688 — said — composer.pickFiles — attach click; file picker failed
- components/chat/conversation-body.tsx:480 — logged — conversation.subscribeOutbox — background subscription
- components/chat/conversation-body.tsx:519 — logged — conversation.subscribeTyping — background subscription
- components/chat/conversation-body.tsx:522 — logged — conversation.markRead — automatic read marker on view
- components/chat/conversation-body.tsx:561 — logged — conversation.subscribePagination — background subscription
- components/chat/conversation-body.tsx:607 — logged — conversation.markRead — debounced automatic read marker
- components/chat/conversation-body.tsx:655 — logged — conversation.listenDragDrop — listener attach
- components/chat/conversation-body.tsx:796 — logged — conversation.setTyping — typing notice fired by keystrokes
- components/chat/conversation-body.tsx:917 — said — conversation.retrySend — Retry click
- components/chat/conversation-body.tsx:965 — said — conversation.cancelSend — Cancel of an in-flight media send
- components/chat/conversation-body.tsx:1056 — said — conversation.toggleReaction — reaction pick or pill click
- components/cheat-sheet/cheat-sheet-overlay.tsx:52 — logged — cheatSheet.sections — read on open; the empty state is honest
- components/export/export-announce.ts:60 — said — export.reveal — Reveal click on the toast
- components/export/export-dialog.tsx:101 — said — export.reveal — Reveal click on the toast
- components/export/export-dialog.tsx:164 — said — export.pickDestination — folder picker failed (cancel resolves null, it does not throw)
- components/export/export-dialog.tsx:199 — said — export.cancel — Cancel export click
- components/export/export-dialog.tsx:346 — said — export.reveal — Reveal in Finder button
- components/files/files-phone-pane.tsx:700 — said — files.reveal — Reveal click
- components/layout/chat-list-pane.tsx:245 — logged — inbox.clearSpaceFilter — automatic clean-up of a stale Space
- components/layout/chat-list-pane.tsx:260 — logged — inbox.clearNetworkFilter — automatic clean-up of a stale Network
- components/layout/chat-list-pane.tsx:276 — logged — inbox.reapplySpaceFilter — automatic re-apply after resubscribe
- components/layout/chat-list-pane.tsx:282 — logged — inbox.reapplyNetworkFilter — automatic re-apply after resubscribe
- components/layout/chat-list-pane.tsx:324 — logged — inbox.listDrafts — draft-marker seeding
- components/layout/chat-list-pane.tsx:346 — logged — inbox.unsubscribeDraftMirror — clean-up after a late subscribe
- components/layout/chat-list-pane.tsx:351 — logged — inbox.subscribeDraftMirror — background subscription
- components/layout/chat-list-pane.tsx:355 — logged — inbox.unsubscribeDraftMirror — unsubscribe on unmount
- components/layout/chat-list-pane.tsx:391 — said — inbox.clearSpaceFilter — Space chip ✕ / Esc; otherwise the list stays filtered after the chip is gone
- components/layout/chat-list-pane.tsx:397 — said — inbox.clearNetworkFilter — Network chip ✕ / Esc
- components/layout/chat-list-pane.tsx:465 — said — chat.archive / chat.unarchive — `e` key
- components/layout/chat-list-pane.tsx:470 — said — chat.pin / chat.unpin — `p` key
- components/layout/chat-list-pane.tsx:475 — said — chat.favorite / chat.unfavorite — `f` key
- components/layout/chat-list-pane.tsx:487 — said — chat.notifyMode — `m` key
- components/layout/conversation-pane.tsx:164 — said — incognito.setChat — Incognito popover choice
- components/layout/conversation-pane.tsx:224 — said — incognito.releaseReceipt — "Mark read publicly" click
- components/layout/favorites-section.tsx:62 — logged — favorites.setCollapsed — persisting the fold; the fold itself already happened on screen
- components/layout/favorites-section.tsx:115 — said — chat.unfavorite — Unfavorite menu item
- components/layout/favorites-section.tsx:142 — logged — favorites.getCollapsed — hydrate; keeps the expanded default
- components/layout/files-pane.tsx:1438 — logged — files.refreshProfiles — profile re-list on Refresh; the last answer stands
- components/layout/files-pane.tsx:1450 — said — files.copyPath — Copy path
- components/layout/files-pane.tsx:2570 — said — files.open — Open menu verb
- components/layout/files-pane.tsx:2688 — said — files.reveal — Reveal menu verb
- components/layout/networks-group.tsx:98 — said — inbox.clearNetworkFilter — row click toggling the filter off
- components/layout/networks-group.tsx:101 — said — inbox.setNetworkFilter — row click selecting a Network
- components/layout/panel-strip.tsx:263 — logged — panel.syncTasks — optional task-name lookup that falls back to null (handler logs then returns null; call order kept)
- components/layout/pins-strip.tsx:81 — said — pins.reorder — drag / Move up-down reorder
- components/layout/pins-strip.tsx:381 — said — chat.unpin — Unpin menu item
- components/layout/spaces-group.tsx:55 — said — inbox.clearSpaceFilter — row click toggling the filter off
- components/layout/spaces-group.tsx:59 — said — inbox.setSpaceFilter — row click selecting a Space
- components/layout/sync-pane.tsx:297 — logged — sync.footprint — the card's status line already says it
- components/layout/sync-pane.tsx:798 — logged — sync.listSizes — the fallbacks match the Rust defaults
- components/layout/sync-pane.tsx:1856 — said — sync.pick — picker failed (cancel resolves null)
- components/layout/tasks-pane.tsx:2484 — logged — tasks.listSizes — same as the Sync pane
- components/notes/capture-settings.tsx:118 — logged — captureSettings.templates — list read; the stored value is still shown
- components/notes/editor/media-chip.ts:203 — said — notes.revealMedia — Reveal button
- components/notes/editor/media-chip.ts:209 — said — notes.copyMediaPath — Copy path button
- components/notes/editor/recording-embed.ts:123 — said — notes.revealRecording — Reveal button
- components/notes/editor/recording-embed.ts:129 — said — notes.copyRecordingPath — Copy path button
- components/notes/media-block-panel.tsx:464 — said — notes.copyMarkerLink — marker "Copy link"
- components/notes/media-recorder-panel.tsx:277 — logged — mediaBlock.linkedNote — falls back to null and the block says "unnamed"
- components/notes/media-recorder-panel.tsx:281 — logged — mediaBlock.recordStarted — same
- components/notes/note-diff-bar.tsx:43 — logged — notes.markRead — acknowledgement; the mark staying set is the visible truth
- components/notes/note-editor.tsx:991 — logged — notes.searchMarks — background search highlighting
- components/notes/note-editor.tsx:1323 — logged — notes.renameFromTitle — background rename while typing; must not interrupt
- components/notes/notes-agent-dock.tsx:440 — logged — agent.focus — automatic focus report
- components/notes/notes-agent-dock.tsx:446 — logged — agent.focus — heartbeat (the inline `if` became a block because the line got too long)
- components/notes/notes-agent-dock.tsx:450 — logged — agent.focus — clean-up on unmount
- components/notes/properties-panel.tsx:2031 — said — notes.revealRecording — Reveal menu item
- components/notes/properties-panel.tsx:2041 — said — notes.previewRecording — Preview menu item
- components/notes/properties-panel.tsx:2056 — said — notes.copyRecordingPath — Copy path menu item
- components/notes/space-editor.tsx:336 — logged — spaceEditor.tagTree — vocabulary read; typing still works
- components/notes/space-editor.tsx:356 — logged — spaceEditor.templates — list read; the stored value is kept
- components/notes/tag-tree.tsx:160 — logged — notes.tagTree — rail section read; the section is just empty
- components/recording/recording-destination-controls.tsx:348 — logged — recording.destinationProfiles — background profile read
- components/recording/recording-destination-controls.tsx:506 — said — recording.pickFolder — folder picker or commit threw ("keeper couldn't change the recording folder.")
- components/recordings/recording-row.tsx:298 — said — recordings.copyId — Copy id
- components/recordings/recordings-pane.tsx:566 — said — recordings.reveal — Reveal
- components/recordings/recordings-pane.tsx:572 — said — recordings.play — Play / open recording
- components/sessions/session-refs.tsx:152 — said — sessions.openRef — click on an external reference
- components/sessions/session-tree.tsx:465 — said — sessions.openFile — open-external button
- components/sessions/session-tree.tsx:481 — said — sessions.revealFile — Reveal button
- components/sessions/sessions-phone-pane.tsx:361 — logged — sessions.listenChanged — listener attach
- components/sessions/space-row-menu.tsx:383: said, `sessions.openFile`. Open in its app is a menu item the person picked.
- components/sessions/space-row-menu.tsx:394: said, `sessions.revealFile`. Reveal is a menu item the person picked.
- components/sessions/space-row-menu.tsx:407: said, `sessions.copyPath`. Copy path is an explicit action; a clipboard refusal used to be silent.
- components/settings/about-section.tsx:95: said, `about.openLink`. A link click whose system opener failed did nothing.
- components/settings/about-section.tsx:247: logged, `about.readAutoUpdate`. Background read; the switch stays disabled, which is the honest state.
- components/settings/bots-section.tsx:879: logged, `bots.readCredentialSource`. Background read; the keychain default stands.
- components/settings/device-verification-dialog.tsx:228: said, `verification.mismatch`. "They don't match" is a click that otherwise does nothing.
- components/settings/device-verification-dialog.tsx:240: said, `verification.confirm`. "They match" is a click that otherwise does nothing.
- components/settings/key-backup-dialog.tsx:171: said, `keyBackup.copy`. Copying the recovery key is an explicit action.
- components/settings/key-backup-dialog.tsx:247: logged, `keyBackup.readSavedKey`. Prefill read; the person can still paste.
- components/settings/no-background-sync-disclosure.tsx:116: logged, `disclosure.markShown`. The card is dismissed locally, and by design the latch never toasts.
- components/settings/phone-push.tsx:71: logged, `push.listenStatus`. Listener; the visibility re-read keeps the status current.
- components/settings/settings-dialog.tsx:568: said, `settings.openAppSettings`. The "Open Settings" button did nothing.
- components/settings/settings-dialog.tsx:1010: logged, `settings.readHotkey`. Read; the section stays in its loading state.
- components/settings/sync-git-row.tsx:120: logged, `syncGit.rereadCapabilities`. The path change succeeded and is shown; the follow-up re-read is a refinement.
- components/settings/telemetry-section.tsx:70: logged, `telemetry.listenStatus`. The polled reads remain the fallback.
- components/settings/telemetry-section.tsx:116: logged, `telemetry.readRemoteConfig`. The shipped fallback is no message; an IPC error is never shown here.
- components/settings/unset-shortcut-row.tsx:83: logged, `shortcut.read`. Read; the row stays in its loading state.
- components/sync/add-folder-form.tsx:1501: logged, `sync.readCredentialSource`. Read; the keychain answer stands.
- components/sync/add-folder-form.tsx:1696: logged, `sync.homeDir`. No home known only turns the Home control off.
- components/sync/add-folder-form.tsx:1725: said, `sync.pickFolder`. A cancel resolves `null`; only a failed picker lands here.
- components/sync/browse-repos-sheet.tsx:541: logged, `forges.relist`. The source re-read right after it carries Rust's sentence.
- components/sync/browse-repos-sheet.tsx:586: logged, `forges.connectCancel`. Cancel on unmount; the surface is gone.
- components/sync/browse-repos-sheet.tsx:603: logged, `forges.connectCancel`. Cancel after unmount; the surface is gone.
- components/sync/browse-repos-sheet.tsx:738: said, `forges.connectCancel`. The Cancel button the person clicked did not cancel.
- components/sync/browse-repos-sheet.tsx:962: said, `forges.openNoticeLink`. A link click whose opener failed.
- components/sync/browse-repos-sheet.tsx:1372: said, `forges.chooseBase`. The folder picker the person asked for failed.
- components/sync/task-form.tsx:1530: said, `tasks.pickFolder`. The folder picker the person asked for failed.
- components/tags/tag-vocabulary-input.tsx:99: logged, `tags.readVocabulary`. Completion is only an aid.
- components/transcription/transcript-player.tsx:368: logged, `transcript.cameraPlay`. The camera only follows the track; the main media plays on.
- components/transcription/transcript-player.tsx:409: logged, `transcript.cameraPlay`. Same follower play, on load.
- components/transcription/transcript-viewer.tsx:456: logged, `transcript.reread`. Background re-read on a write event; the lines on screen stand.
- components/transcription/transcript-viewer.tsx:462: logged, `transcript.listenWritten`. Listener registration.
- components/wizard/first-run-wizard.tsx:125: logged, `wizard.readSkipSetup`. Read; the unchecked box is the honest default.
- hooks/use-account-mirror.ts:50: logged, `account.listenSetup`. Listener; the subscription still runs.
- hooks/use-account-mirror.ts:56: logged, `account.unsubscribe`. Cleanup after cancel.
- hooks/use-account-mirror.ts:60: logged, `account.subscribe`. The store keeps its honest "no account" default.
- hooks/use-account-mirror.ts:73: logged, `account.sync`. Return-triggered poke; Rust's sentence arrives over the subscription.
- hooks/use-account-mirror.ts:92: logged, `account.unsubscribe`. Unmount cleanup.
- hooks/use-account-statuses.ts:64: logged, `accountStatus.subscribe`. Stream; the account stays pending.
- hooks/use-active-chat-reporter.ts:60: logged, `chat.reportActive`. Automatic active-chat report.
- hooks/use-app-lifecycle.ts:73: logged, `app.lifecycle`. Automatic lifecycle report.
- hooks/use-auto-update.ts:133: logged, `update.readPlan`. Background; the last plan is kept.
- hooks/use-auto-update.ts:155: logged, `update.relaunch`. Background relaunch nobody is watching, by construction.
- hooks/use-auto-update.ts:202: logged, `update.readPlan`. Background; the last plan is kept.
- hooks/use-auto-update.ts:258: logged, `update.readPlan`. Opening read; the manual About button stands.
- hooks/use-bbctl-run.ts:57: logged, `bbctl.cancel`. Best-effort cancel; the sheet closes.
- hooks/use-bbctl-run.ts:83: logged, `bbctl.cancel`. Cancel after teardown.
- hooks/use-bbctl-run.ts:106: logged, `bbctl.cancel`. Unmount cleanup.
- hooks/use-bridge-health.ts:57: logged, `bridgeHealth.subscribe`. Stream; no health is shown.
- hooks/use-bridge-login.ts:58: logged, `bridgeLogin.submit`. The stream has already rendered the terminal state.
- hooks/use-bridge-login.ts:70: logged, `bridgeLogin.cancel`. Best-effort cancel.
- hooks/use-bridge-login.ts:97: logged, `bridgeLogin.cancel`. Cancel after teardown.
- hooks/use-bridge-login.ts:124: logged, `bridgeLogin.cancel`. Unmount cleanup.
- hooks/use-capture-draft.ts:152: logged, `capture.listenShown`. Listener; `dismiss` re-arms on the path that matters.
- hooks/use-encryption-statuses.ts:62: logged, `encryptionStatus.subscribe`. Stream; the account stays pending.
- hooks/use-global-hotkey.ts:40: logged, `hotkey.listen`. Listener registration.
- hooks/use-key-backup-statuses.ts:58: logged, `keyBackupStatus.subscribe`. Stream; the account stays pending.
- hooks/use-menu-actions.ts:95: logged, `menu.listen`. Listener registration.
- hooks/use-nav-state-persistence.ts:93: logged, `navState.clear`. Automatic nav persistence.
- hooks/use-nav-state-persistence.ts:95: logged, `navState.set`. Automatic nav persistence.
- hooks/use-nav-state-persistence.ts:126: logged, `navState.read`. Restore read; still folds to `null` (cold launch).
- hooks/use-notes-actions.ts:352: logged, `notes.spaceEntry`. Queue tail; the caller holds `entry` and says its failure.
- hooks/use-notes-body.ts:132: said (when unsaved words rode the close) or logged (no text), `notes.release`. A close carrying dirty text that failed means those words never reached disk.
- hooks/use-notes-body.ts:269: logged, `notes.bufferReport`. Heartbeat sent on every keystroke idle; no toast spam.
- hooks/use-notes-body.ts:308: logged, `notes.close`. Orphan subscription cleanup.
- hooks/use-notes-changes.ts:122: logged, `notes.subscribeChanges`. Stream; the loaded query stands.
- hooks/use-notes-open-note.ts:58: said, `notes.openNote`. The person asked to open a note and it would silently not show.
- hooks/use-notes-open-note.ts:71: logged, `notes.listenOpenNote`. Listener registration.
- hooks/use-notify-navigate.ts:64: logged, `notify.listenNavigate`. Listener registration.
- hooks/use-push-open.ts:32: said, `push.open`. A tapped push that opens nothing.
- hooks/use-push-open.ts:46: logged, `push.listenOpen`. Listener registration.
- hooks/use-recording-hotkey.ts:47: logged, `recording.listenHotkey`. Listener registration.
- hooks/use-recording-permission.ts:153: said, `recording.requestMicrophone`. Explicit request click; the OS prompt never came.
- hooks/use-recording-permission.ts:163: said, `recording.requestCamera`. Explicit request click.
- hooks/use-recording-permission.ts:172: said, `recording.openSettings`. Deep-link button.
- hooks/use-recording-permission.ts:176: said, `recording.openMicrophoneSettings`. Deep-link button.
- hooks/use-recording-permission.ts:180: said, `recording.openCameraSettings`. Deep-link button.
- hooks/use-recording-removed.ts:31: logged, `recording.forgetInNote`. Background; still folds to `null` (the note is left alone).
- hooks/use-recording-removed.ts:53: logged, `recording.listenRemoved`. Listener registration.
- hooks/use-recording-session.ts:176: logged, `recording.status`. Mount read; the idle default stands.
- hooks/use-recording-session.ts:206: logged, `recording.status`. 1 s poll; the last snapshot stands.
- hooks/use-recording-session.ts:285: said, `recording.stop`. Stop was asked for; a refused stop leaves the recording running.
- hooks/use-recording-session.ts:297: said, `recording.acknowledge`. A dismiss click that did not dismiss.
- hooks/use-recovered-sessions.ts:75: logged, `recording.listRecovered`. Refresh; the previous list stands.
- hooks/use-recovered-sessions.ts:90: logged, `recording.acknowledgeRecovered`. The card is already dropped locally; the latch is best-effort.
- hooks/use-session-restore.ts:30: logged, `session.restore`. Falls through to the login screen.
- hooks/use-sessions-search.ts:90: logged, `sessions.searchCancel`. Cancel after cleanup.
- hooks/use-sessions-search.ts:107: logged, `sessions.searchCancel`. Unmount cleanup.
- hooks/use-spoken-stream.ts:51: logged, `bots.listenSpokenStream`. Listener registration.
- hooks/use-verification.ts:65: logged, `verification.accept`. Automatic accept; failure surfaces as a terminal state over the stream.
- hooks/use-verification.ts:84: logged, `verification.subscribe`. Stream; there is simply no auto-open.
- hooks/use-voice-facts.ts:45: logged, `voice.readWake`. Read; there is no switch to draw.
- hooks/use-voice-facts.ts:61: logged, `voice.readAvailability`. Read; stays `undefined`.
- hooks/use-voice-facts.ts:103: logged, `voice.listenWake`. Listener registration.
- hooks/use-voice-stream.ts:77: logged, `voice.watch`. Stream; the chip stays off.
- lib/agents/surface.ts:181: logged, `agents.surfaceResult`. The agent stopped waiting; nobody is left to tell.
- lib/agents/surface.ts:241: logged, `agents.surfaceVault`. Agent-driven, not a person's action.
- lib/agents/surface.ts:499: logged, `agents.surfaceSubscribe`. Startup subscription.
- lib/stores/capture-windows.ts:87: logged, `capture.listWindows`. Re-read; the last list stays, deliberately.
- lib/stores/forges.ts:40: logged, `forges.list`. Re-read; what was held is kept.
- lib/stores/incognito.ts:101: logged, `incognito.read`. Mirror refresh; the last VM stays.
- lib/stores/notes-filters.ts:436: logged, `notes.persistHideServiceFiles`. Queue tail; the caller holds `write` (rolled back, rethrown).
- lib/stores/notes-filters.ts:472: logged, `notes.persistIncludePrivate`. Queue tail; same as above.
- lib/stores/recording-capture-sources.ts:177: said, `recording.rememberSources`. The person toggled a source and the choice will not survive a relaunch.
- lib/stores/recording-settings.ts:157: logged, `recording.refreshSettings`. Refinement read; the value on screen stays.
- lib/stores/recording-source.ts:167: logged, `recording.listSources`. Poll; the prior snapshot stays.
- lib/stores/sync-detail.ts:331: logged, `sync.unsubscribeProgress`. Cleanup after teardown.
- lib/stores/sync-detail.ts:336: logged, `sync.subscribeProgress`. Stream; the polled status stays honest.
- lib/stores/sync-detail.ts:343: logged, `sync.unsubscribeProgress`. Teardown cleanup.
- lib/stores/verification.ts:49: logged, `verification.cancel`. Cancel on modal close; the modal closes regardless.
- lib/titlebar-drag.ts:52: logged, `titlebar.report`. A diagnostic report the window layer refused.
- lib/viewers/unknown-viewer.tsx:182: said, `viewer.openWith`. The "Open with" button did nothing.
- lib/viewers/unknown-viewer.tsx:194: said, `viewer.reveal`. The Reveal button did nothing.
- study/session.ts:38: logged (study-local), `study.stop`. Abort cleanup; the error is already redacted.
- study/session.ts:43: logged (study-local), `study.stop`. Failure cleanup; `onFailure` shows the notice.
- study/session.ts:84: logged (study-local), `study.stop`. The redacted start failure is still what gets thrown.
- study/study.tsx:31: logged (study-local), `study.preview`. Disclosure read.
- study/study.tsx:42: logged (study-local), `study.start`. The start's own catch already shows a notice; this only decides what to stop. Still folds to `null`.
- study/study.tsx:49: logged (study-local), `study.closeStop`. Close handler; the document is replaced either way.
- study/study.tsx:129: logged (study-local), `study.start`. Same as :42, on the Stop path.

## Other changes and notes

- components/account/setup-code-scanner.test.tsx — added `frontendErrorReport` to the `@/lib/ipc/client` mock; the camera-settings failure test now expects a log of `setup.openCameraSettings`. Without the mock entry, the failure path would throw "no export defined".
- components/bots/bot-answer.test.tsx — mocked the client with `importOriginal` plus a `frontendErrorReport` spy (top-level `import type * as IpcClient`); "says nothing" became "says and logs" (`bots.copyCode`).
- components/notes/capture-settings.test.tsx — added `frontendErrorReport` to the mock; the failed-list test asserts `captureSettings.templates` is logged.
- components/notes/space-editor.test.tsx — same, asserting `spaceEditor.templates`.
- components/recording/recording-destination-controls.test.tsx — same; the picker-throws test asserts `recording.pickFolder` is logged.
- components/recordings/recording-row.test.tsx — same; "swallows a clipboard" became "says and logs" (`recordings.copyId`).
- bot-message-meta.test and bot-voice-target.test use `importOriginal`, so the real async `frontendErrorReport` rejects and that rejection is caught.
- In files-pane.test, the `syncProfiles` rejections hit the first load, not Refresh.
- approval-pane, chat-row, conversation-pane, notes-agent-dock and App tests don't exercise the changed failure paths.
- A throwaway script ran the guard's own AST logic over the 46 files and found 0 silent catches and 0 parse errors. It also checked that each `@/lib/failure` import matches what the file uses, and that no changed line is over 100 characters.
- The 6 changed test files parse cleanly with TypeScript.
- Not run, per the brief: tsc, biome and vitest. Those are the parent's gates.
- hooks/use-telemetry.ts:17: NOT EDITED. The file is on the do-not-touch list, and the parent has already replaced it with `logFailure("telemetry.capture")`. The scan shows it clean.
- lib/crash-report.ts:31: console.error, not routed through `logFailure`/`sayFailure`. The log funnel itself refused, so the console is the only witness left; another report would loop back here. The funnel now also catches a synchronous throw from the wrapper, so it keeps its documented "never thrown" promise.
- voice-main.tsx:41: console.error, not routed through `logFailure`/`sayFailure`. The voice window's capability grants no command (`frontend_error_report` included), and the entry is meant to import nothing, so the console is the only place the failure can go.
- `study/failure.ts`: `logStudyFailure(source)` calls `frontend_error_report` through a raw `invoke`. `study/commands.ts` says the study entry imports no app client or store, and `@/lib/failure` pulls in both.
- hooks/use-app-lifecycle.test.ts: mock gains `frontendErrorReport`. "swallows an IPC rejection" becomes "logs … to keeper.log" and asserts the `app.lifecycle` report.
- hooks/use-auto-update.test.ts: mock gains it. The refused-relaunch test asserts the `update.relaunch` report. Without the mock, the throw would have ended the restart loop.
- hooks/use-nav-state-persistence.test.ts: mock gains it. The rejected-read test asserts the `navState.read` report.
- hooks/use-encryption-statuses.test.ts: "swallows" becomes "logs" and asserts the report.
- hooks/use-key-backup-statuses.test.ts: same change as encryption-statuses.
- hooks/use-verification.test.ts: same change as encryption-statuses.
- hooks/use-voice-stream.test.ts: mock gains it. The failed-availability test asserts the `voice.readAvailability` report.
- hooks/use-session-restore.test.ts: mock gains it. The rejected-restore test asserts the `session.restore` report.
- hooks/use-recovered-sessions.test.ts: mock gains it, because the failed-refresh test now reaches the log.
- hooks/use-recording-session.test.ts: mock gains it, and `sonner` is mocked. The failed-acknowledge test asserts the toast carries Rust's sentence.
- These failure-path tests only needed the mock entry (no new assertion):
- components/notes/space-list.test.tsx: mock entry only. This file is outside the batch, but its refused-park test reaches the `notes.spaceEntry` queue tail.

## Fix round 1 re-sweep (R387, FIX 08)

The first sweep fixed empty catches. This one scanned for catch handlers that do have a body but never call `logFailure`/`sayFailure`/`reportFrontendError`/`logStudyFailure`/`toast.error`: promise `.catch(handler)` and `try { await … } catch {}` blocks under `src/` (scan `/tmp/agents-salvage/scan-100errors-f1.txt`). It found 372. Most show an inline error, set a failed/empty/loading state, or fall back to a default the person sees, without writing anything to keeper.log. 359 now report at the top of the handler (one of them, composer.tsx:onSend, done by the coordinator). Every existing guard, setter, fallback, retry and rethrow is unchanged: a parameterless handler gained `(raw: unknown)`, and a binding-less `catch {` gained a name that does not shadow anything its body reads. Two report on the failure branch only, so an answer that is not a failure (an OAuth cancel, a "file already there" question) is not logged. 13 are left, each with its reason. In the table, `class` is what the handler does on screen, or why it was left. `done` is the source name the handler now logs under, or the reason it was left.

| site (file:symbol) | class | done |
| --- | --- | --- |
| components/chat/composer.tsx:flushDraft | stuck state | draft not saved → one inline line DRAFT_NOT_SAVED, draft unmarked, `logFailure("composer.saveDraft")` (coordinator) |
| components/settings/unset-shortcut-row.tsx:UnsetShortcutRow read | stuck state | stuck loading → alert with Rust's sentence, `logFailure("shortcut.read")`; writes `logFailure("shortcut.write")` (coordinator) |
| hooks/use-recording-permission.ts:refresh | stuck state | probe failure → `failure` sentence under Permissions in recording-pane and media-recorder-panel, `logFailure("recording.permissionProbe")` (coordinator) |
| hooks/use-recording-permission.ts:request | inline error | `sayFailure("recording.requestScreenRecording")` (coordinator) |
| hooks/use-notes-body.ts:writeNote | inline error | inline save bar kept + `logFailure("notes.save")` (coordinator) |
| components/bots/bot-voice-wake.tsx:save/chooseLocale | inline error | inline refusal kept + `logFailure("voice.wakeSet")`/`logFailure("voice.localeSet")` (coordinator) |
| App.tsx:App posture read | fallback | `logFailure("app.encryptionPosture")` |
| App.tsx:App setupSkipped read | fallback | `logFailure("app.setupSkippedRead")` |
| components/account/account-share-sheet.tsx:ShareBody | inline error | `logFailure("account.share")` |
| components/account/setup-code-scanner.tsx:SetupCodeScanner camera start | inline error | `logFailure("setup.camera")` |
| components/account/setup-code-scanner.tsx:SetupCodeScanner tick | inline error | `logFailure("setup.scanFrame")` |
| components/agents/approval-card.tsx:decide | inline error | `logFailure("agents.approvalDecide")` |
| components/agents/approval-card.tsx:denyAll | inline error | `logFailure("agents.approvalDenyAll")` |
| components/agents/approval-card.tsx:toggle | inline error | `logFailure("agents.approvalPayload")` |
| components/approval/approval-pane.tsx:requery | inline error | `logFailure("approval.listDrafts")` |
| components/auth/login-screen.tsx:handleOidc | inline error | `logFailure("login.oidc")` — reported in the non-cancel branch only: a user cancel is not a failure |
| components/auth/login-screen.tsx:handleSendCode | inline error | `logFailure("login.beeperRequestCode")` |
| components/auth/login-screen.tsx:handleSubmit | inline error | `logFailure("login.password")` |
| components/auth/login-screen.tsx:handleVerify | inline error | `logFailure("login.beeper")` |
| components/bots/bot-approval-dialog.tsx:always | inline error | `logFailure("bots.approvalGrantSave")` |
| components/bots/bot-attachment.tsx:BotDeliverables paths | fallback | `logFailure("bots.deliverablePaths")` |
| components/bots/bot-attachment.tsx:handle | inline error | `logFailure("bots.imagePaste")` |
| components/bots/bot-composer.tsx:BotComposer preview | fallback | `logFailure("bots.commandPreview")` |
| components/bots/bot-composer.tsx:submit | inline error | `logFailure("bots.commandResolve")` |
| components/bots/bot-composer.tsx:submit help | fallback | `logFailure("bots.commandHelp")` |
| components/bots/bot-conversation.tsx:BotConversation follow | stuck state | `logFailure("bots.sessionFollow")` |
| components/bots/bot-grant-bar.tsx:GrantForm save | inline error | `logFailure("bots.grantSave")` |
| components/bots/bot-grant-bar.tsx:refresh | inline error | `logFailure("bots.grantsList")` |
| components/bots/bot-grant-bar.tsx:revoke | inline error | `logFailure("bots.grantRevoke")` |
| components/bots/bot-identity.tsx:save | inline error | `logFailure("bots.identitySave")` |
| components/bots/bot-picker.tsx:BotPicker models read | inline error | `logFailure("bots.modelsList")` |
| components/bots/bot-voice-mic.tsx:start | inline error | `logFailure("voice.start")` |
| components/bots/bot-voice-target.tsx:BotVoiceTarget bots read | fallback | `logFailure("voice.botsList")` |
| components/bots/bot-voice-target.tsx:choose | inline error | `logFailure("voice.targetSet")` |
| components/bots/bots-pane.tsx:onStreamEvent | inline error | `logFailure("bots.approvalAnswer")` |
| components/bots/bots-pane.tsx:openConversation | inline error | `logFailure("bots.sessionOpen")` |
| components/bots/bots-pane.tsx:retry | inline error | `logFailure("bots.retry")` |
| components/bots/bots-pane.tsx:send | inline error | `logFailure("bots.send")` |
| components/bots/bots-phone-pane.tsx:open | inline error | `logFailure("bots.sessionOpen")` |
| components/bots/bots-phone-pane.tsx:retry | inline error | `logFailure("bots.retry")` |
| components/bots/bots-phone-pane.tsx:send | inline error | `logFailure("bots.send")` |
| components/bridges/bbctl-panel.tsx:BbctlPanel availability | inline error | `logFailure("bbctl.availability")` |
| components/chat/chat-row.tsx:onMarkRead | fallback | `logFailure("chat.markRead")` |
| components/chat/chat-row.tsx:onMarkUnread | fallback | `logFailure("chat.markUnread")` |
| components/chat/composer.tsx:onSend | inline error | `logFailure("composer.send")` (coordinator) |
| components/chat/conversation-body.tsx:ConversationBody subscribe | inline error | `logFailure("chat.subscribeTimeline")` |
| components/chat/conversation-body.tsx:runPaginate | inline error | `logFailure("chat.paginate")` |
| components/chat/conversation-body.tsx:tryLand paginate | fallback | `logFailure("chat.deepLinkPaginate")` |
| components/chat/conversation-body.tsx:tryLand resolve | fallback | `logFailure("chat.deepLinkResolve")` |
| components/chat/delete-message-dialog.tsx:DeleteMessageDialog label probe | fallback | `logFailure("chat.networkLabel")` |
| components/chat/delete-message-dialog.tsx:handleConfirm | inline error | `logFailure("chat.deleteMessage")` |
| components/chat/edit-history-popover.tsx:onOpenChange | inline error | `logFailure("chat.editHistory")` |
| components/chat/new-chat-dialog.tsx:NewChatDialog support read | fallback | `logFailure("newChat.resolveSupport")` |
| components/chat/new-chat-dialog.tsx:resolve | inline error | `logFailure("newChat.resolve")` |
| components/command-palette/actions.ts:recording-open-folder | stuck state | `logFailure("palette.revealRecordings")` |
| components/command-palette/command-palette.tsx:CommandPalette query | fallback | `logFailure("palette.query")` |
| components/export/export-dialog.tsx:onStart | inline error | `logFailure("export.start")` |
| components/files/files-phone-pane.tsx:FilesPhonePane profiles read | inline error | `logFailure("phoneFiles.profiles")` |
| components/files/files-phone-pane.tsx:load | inline error | `logFailure("phoneFiles.browse")` |
| components/files/files-phone-pane.tsx:refresh | inline error | `logFailure("phoneFiles.syncNow")` |
| components/files/files-phone-pane.tsx:share | inline error | `logFailure("phoneFiles.share")` |
| components/files/files-phone-pane.tsx:withContent | inline error | `logFailure("phoneFiles.materialize")` |
| components/layout/account-footer.tsx:AccountIncognitoSubmenu onSelect | fallback | `logFailure("incognito.setAccount")` |
| components/layout/account-footer.tsx:AccountIncognitoSubmenu read | fallback | `logFailure("incognito.getAccount")` |
| components/layout/account-footer.tsx:GlobalDndToggle onToggle | fallback | `logFailure("dnd.setGlobal")` |
| components/layout/account-footer.tsx:GlobalDndToggle read | fallback | `logFailure("dnd.getGlobal")` |
| components/layout/account-footer.tsx:handleDeleteArchiveConfirm | inline error | `logFailure("account.signOutDeleteArchive")` |
| components/layout/account-footer.tsx:handleKeepArchiveConfirm | stuck state | `logFailure("account.signOut")` |
| components/layout/chat-list-pane.tsx:ChatListPane subscribe | inline error | `logFailure("inbox.subscribe")` |
| components/layout/chat-list-pane.tsx:runVerb u | fallback | `logFailure("chat.markReadToggle")` |
| components/layout/files-pane.tsx:confirmDelete | inline error | `logFailure("files.delete")` |
| components/layout/files-pane.tsx:createFile | inline error | `logFailure("files.create")` |
| components/layout/files-pane.tsx:FilesPane profiles read | fallback | `logFailure("files.profiles")` |
| components/layout/files-pane.tsx:load | inline error | `logFailure("files.browse")` |
| components/layout/files-pane.tsx:requestDelete | inline error | `logFailure("files.deletePlan")` |
| components/layout/files-pane.tsx:runRowVerb | inline error | `logFailure("files.rowVerb")` |
| components/layout/networks-group.tsx:NetworkRow mute read | fallback | `logFailure("networks.muteGet")` |
| components/layout/networks-group.tsx:onToggleMute | fallback | `logFailure("networks.muteSet")` |
| components/layout/panel-strip.tsx:RunLogPanel load | inline error | `logFailure("panel.runLog")` |
| components/layout/panel-strip.tsx:useFileResolution | inline error | `logFailure("panel.resolveFile")` |
| components/layout/panel-strip.tsx:useRunResolution | inline error | `logFailure("panel.resolveRun")` |
| components/layout/phone-search-surface.tsx:PhoneSearchSurface query | fallback | `logFailure("phoneSearch.palette")` |
| components/layout/phone-shell.tsx:PhoneShell pull-to-refresh | stuck state | `logFailure("phone.pullRefresh")` |
| components/layout/recording-summary-card.tsx:loadDetails | inline error | `logFailure("recording.sessionMeta")` |
| components/layout/recording-summary-card.tsx:save | inline error | `logFailure("recording.retitle")` |
| components/layout/sync-pane.tsx:run | inline error | `logFailure("syncPane.action")` |
| components/layout/tasks-pane.tsx:forget | inline error | `logFailure("tasks.forget")` |
| components/layout/tasks-pane.tsx:forgetSelection | inline error | `logFailure("tasks.forgetSelection")` |
| components/layout/tasks-pane.tsx:readHistory | inline error | `logFailure("tasks.history")` |
| components/layout/tasks-pane.tsx:runNow | inline error | `logFailure("tasks.runNow")` |
| components/layout/tasks-pane.tsx:RunRow pathAction | inline error | `logFailure("tasks.runLogPath")` |
| components/layout/tasks-pane.tsx:setSelectionEnabled | inline error | `logFailure("tasks.setEnabled")` |
| components/layout/tasks-pane.tsx:TaskFolderRow reveal | inline error | `logFailure("tasks.revealFolder")` |
| components/layout/tasks-pane.tsx:TaskRow reveal | inline error | `logFailure("tasks.reveal")` |
| components/layout/tasks-pane.tsx:toggleTaskEnabled | inline error | `logFailure("tasks.toggleEnabled")` |
| components/notes/adopt-media-blocks.tsx:adopt | inline error | `logFailure("media.adopt")` |
| components/notes/attach-file-button.tsx:pick | inline error | `logFailure("notes.attachFile")` |
| components/notes/attach-from-vault-dialog.tsx:AttachFromVaultDialog listing | inline error | `logFailure("notes.gallery")` |
| components/notes/attach-to-note-dialog.tsx:attach | inline error | `logFailure("notes.attachToNote")` |
| components/notes/attach-to-note-dialog.tsx:AttachToNoteDialog search | inline error | `logFailure("notes.attachTargets")` |
| components/notes/attachments-panel.tsx:AttachmentsPanel embed paths | fallback | `logFailure("notes.embedPaths")` |
| components/notes/attachments-panel.tsx:AttachmentsPanel session targets | fallback | `logFailure("notes.recordingTargets")` |
| components/notes/board-card-agent.tsx:allow | inline error | `logFailure("notes.boardAllow")` |
| components/notes/capture-settings.tsx:CaptureSettings impact | fallback | `logFailure("captureSettings.impact")` |
| components/notes/capture-settings.tsx:save | inline error | `logFailure("captureSettings.save")` |
| components/notes/conflict-resolver.tsx:finish | inline error | `logFailure("notes.resolveConflict")` |
| components/notes/editor/csv-table.ts:commit | inline error | `logFailure("notes.csvSetCell")` |
| components/notes/editor/csv-table.ts:renderCsvTableInto | inline error | `logFailure("notes.csvRead")` |
| components/notes/editor/gallery-block.ts:renderGalleryInto | inline error | `logFailure("notes.galleryBlock")` |
| components/notes/editor/markdown-table.ts:readCsvAsTable | inline error | `logFailure("notes.csvToTable")` |
| components/notes/editor/markdown-table.ts:writeTableAsCsv | inline error | `logFailure("notes.tableToCsv")` — reported on the refusal path only; the "a file is already there" question is an answer, not a failure |
| components/notes/editor/media-block.ts:open | fallback | `logFailure("media.blockHostLoad")` |
| components/notes/editor/media-chip.ts:playInPlayer | inline error | `logFailure("media.playInPlayer")` |
| components/notes/editor/media-hints.ts:mediaBlockCompleteSource ask | fallback | `logFailure("notes.mediaHintAsk")` |
| components/notes/editor/media-hints.ts:mediaBlockCompleteSource schema | fallback | `logFailure("notes.mediaSchema")` |
| components/notes/editor/media-hints.ts:run | fallback | `logFailure("notes.mediaCheck")` |
| components/notes/editor/mermaid-widget.ts:renderMermaidInto | not IPC | left: mermaid parsing/rendering the fence's own text in the webview; the degraded block already shows the fence and the reason |
| components/notes/editor/note-widget-host.tsx:NoteWidget read | inline error | `logFailure("notes.widget")` |
| components/notes/editor/recording-embed.ts:renderRecordingEmbedInto | fallback | `logFailure("notes.recordingEmbedTargets")` |
| components/notes/editor/vault-embed.ts:renderVaultEmbedInto | fallback | `logFailure("notes.embedResolve")` |
| components/notes/links-panel.tsx:LinksPanel read | fallback | `logFailure("notes.links")` |
| components/notes/media-block-panel.tsx:edit | inline error | `logFailure("media.blockEdit")` |
| components/notes/media-picker-dialog.tsx:compose | inline error | `logFailure("media.blockCompose")` |
| components/notes/note-delete-dialog.tsx:confirm | inline error | `logFailure("notes.delete")` |
| components/notes/note-delete-dialog.tsx:NoteDeleteDialog plan | inline error | `logFailure("notes.deletePlan")` |
| components/notes/note-filter-bar.tsx:createFromSearch | inline error | `logFailure("notes.createFromSearch")` |
| components/notes/note-filter-bar.tsx:Include private notes button | inline error | `logFailure("notes.includePrivateSet")` |
| components/notes/note-filter-bar.tsx:NoteFilterBar tags read | inline error | `logFailure("notes.tagsVocabulary")` |
| components/notes/note-history-panel.tsx:NoteHistoryPanel diff read | fallback | `logFailure("notes.diff")` |
| components/notes/note-history-panel.tsx:NoteHistoryPanel history read | inline error | `logFailure("notes.history")` |
| components/notes/note-history-panel.tsx:Restore button | inline error | `logFailure("notes.restoreRevision")` |
| components/notes/notes-agent-dock.tsx:AskAssistant ask | inline error | `logFailure("notes.agentConversationNew")` |
| components/notes/notes-agent-dock.tsx:relist | inline error | `logFailure("notes.agentRooms")` |
| components/notes/notes-agent-dock.tsx:ScopeAsk apply | inline error | `logFailure("notes.agentScopeSet")` |
| components/notes/notes-pane.tsx:NotesPane search subscribe | inline error | `logFailure("notes.subscribeSearch")` |
| components/notes/notes-pane.tsx:NotesPane visibility hydrate | inline error | `logFailure("notes.hydrateVisibility")` |
| components/notes/notes-phone-pane.tsx:NotesPhonePane search subscribe | inline error | `logFailure("notes.subscribeSearch")` |
| components/notes/notes-phone-pane.tsx:NotesPhonePane visibility hydrate | inline error | `logFailure("notes.hydrateVisibility")` |
| components/notes/physical-tree.tsx:FolderNode read | fallback | `logFailure("notes.tree")` |
| components/notes/physical-tree.tsx:PhysicalTree root read | fallback | `logFailure("notes.treeRoot")` |
| components/notes/properties-panel.tsx:FieldControl load | fallback | `logFailure("properties.fieldVocabulary")` |
| components/notes/properties-panel.tsx:FileProperties read | fallback | `logFailure("properties.readFrontmatter")` |
| components/notes/properties-panel.tsx:PropertiesPanel session targets | fallback | `logFailure("notes.recordingTargets")` |
| components/notes/properties-panel.tsx:Record another button | inline error | `logFailure("properties.recordAnother")` |
| components/notes/properties-panel.tsx:TagsControl vocabulary | fallback | `logFailure("properties.tagsVocabulary")` |
| components/notes/properties-panel.tsx:write | inline error | `logFailure("properties.write")` |
| components/notes/search-field.tsx:SearchField vocabulary read | inline error | `logFailure("notes.tagsVocabulary")` |
| components/notes/search-settings.tsx:embedding model select | inline error | `logFailure("notes.embeddingModelSet")` |
| components/notes/search-settings.tsx:saveNames | inline error | `logFailure("notes.serviceFilesSave")` |
| components/notes/search-settings.tsx:SearchSettings embedding models | inline error | `logFailure("notes.embeddingModels")` |
| components/notes/search-settings.tsx:SearchSettings read | inline error | `logFailure("notes.searchSettingsRead")` |
| components/notes/space-editor.tsx:save | inline error | `logFailure("notes.spaceSave")` |
| components/notes/space-editor.tsx:SpaceEditor terms read | fallback | `logFailure("notes.spaceTerms")` |
| components/notes/space-list.tsx:duplicate | inline error | `logFailure("notes.spaceDuplicate")` |
| components/notes/space-list.tsx:pin | inline error | `logFailure("notes.spacePin")` |
| components/notes/space-list.tsx:reload | inline error | `logFailure("notes.spaces")` |
| components/notes/space-list.tsx:restore | inline error | `logFailure("notes.spacesRestore")` |
| components/notes/space-name-popover.tsx:save | inline error | `logFailure("notes.spaceName")` |
| components/notes/task-board.tsx:move | inline error | `logFailure("notes.boardMove")` |
| components/notes/template-update-offer.tsx:apply | inline error | `logFailure("notes.templateUpdateApply")` |
| components/notes/template-update-offer.tsx:TemplateUpdateOffer preview | inline error | `logFailure("notes.templateUpdatePreview")` |
| components/notes/template-update-offer.tsx:Undo button | inline error | `logFailure("notes.templateUpdateUndo")` |
| components/recording/recording-audio-controls.tsx:onMicToggle | fallback | `logFailure("recording.micPermission")` |
| components/recording/recording-destination-controls.tsx:RecordingDestinationControls preview | stuck state | `logFailure("recording.pathPreview")` |
| components/recording/recording-destination-controls.tsx:saveHead | inline error | `logFailure("recording.subfolderSave")` |
| components/recording/recording-note-stub.tsx:commit | inline error | `logFailure("recording.noteStubSave")` |
| components/recording/recording-note-stub.tsx:dismiss | inline error | `logFailure("recording.noteStubDismiss")` |
| components/recording/recording-note-stub.tsx:RecordingNoteStub resolve | fallback | `logFailure("recording.noteStub")` |
| components/recording/recording-webcam-controls.tsx:onCameraToggle | fallback | `logFailure("recording.cameraPermission")` |
| components/recordings/recordings-pane.tsx:reconcileNow | inline error | `logFailure("recordings.reconcileNow")` |
| components/recordings/recordings-pane.tsx:runSearch | inline error | `logFailure("recordings.search")` |
| components/search/search-panel.tsx:SearchPanel query | inline error | `logFailure("search.archive")` |
| components/sessions/promote-panel.tsx:ArtifactOffer promote | inline error | `logFailure("promote.artifact")` |
| components/sessions/promote-panel.tsx:KnowledgeNote promote | inline error | `logFailure("promote.note")` |
| components/sessions/promote-panel.tsx:open | inline error | `logFailure("promote.readNote")` |
| components/sessions/promote-panel.tsx:PromoteRow promote | inline error | `logFailure("promote.file")` |
| components/sessions/promote-panel.tsx:refresh | inline error | `logFailure("promote.table")` |
| components/sessions/promote-panel.tsx:review | inline error | `logFailure("promote.review")` |
| components/sessions/session-actions.tsx:SessionActions archive confirm | inline error | `logFailure("sessions.archive")` |
| components/sessions/session-add-ref.tsx:add | inline error | `logFailure("sessions.refAdd")` |
| components/sessions/session-add-ref.tsx:SessionAddRef candidates | inline error | `logFailure("sessions.refCandidates")` |
| components/sessions/session-file-actions.tsx:create | inline error | `logFailure("sessions.fileNew")` |
| components/sessions/session-file-actions.tsx:createDir | inline error | `logFailure("sessions.dirNew")` |
| components/sessions/session-file-actions.tsx:newKind | inline error | `logFailure("sessions.fileNewKind")` |
| components/sessions/session-file-actions.tsx:newLog | inline error | `logFailure("sessions.logToday")` |
| components/sessions/session-space-editor.tsx:save | inline error | `logFailure("sessions.spaceSave")` |
| components/sessions/session-space-editor.tsx:SessionSpaceEditor terms read | fallback | `logFailure("sessions.spaceTerms")` |
| components/sessions/session-spaces.tsx:confirmDelete | inline error | `logFailure("sessions.spaceDelete")` |
| components/sessions/session-spaces.tsx:narrow | inline error | `logFailure("sessions.spaceNarrow")` |
| components/sessions/session-spaces.tsx:newNote | inline error | `logFailure("sessions.spaceNewNote")` |
| components/sessions/session-spaces.tsx:restore | inline error | `logFailure("sessions.spacesRestore")` |
| components/sessions/session-templates.tsx:confirmDelete | inline error | `logFailure("sessions.templateEntryDelete")` |
| components/sessions/session-templates.tsx:mint | inline error | `logFailure("sessions.templateEntryNew")` |
| components/sessions/session-templates.tsx:rename | inline error | `logFailure("sessions.templateRename")` |
| components/sessions/session-templates.tsx:renameEntry | inline error | `logFailure("sessions.templateEntryRename")` |
| components/sessions/session-tree.tsx:confirmDelete | inline error | `logFailure("sessions.fileDelete")` |
| components/sessions/sessions-pane.tsx:installTemplate | inline error | `logFailure("sessions.templateInstall")` |
| components/sessions/sessions-pane.tsx:SessionsPane patterns read | inline error | `logFailure("sessions.patterns")` |
| components/sessions/sessions-phone-pane.tsx:move | inline error | `logFailure("sessions.taskMove")` |
| components/sessions/sessions-settings.tsx:onChange | fallback | `logFailure("sessions.spacesFoldedSet")` |
| components/sessions/sessions-settings.tsx:SessionsSettings read | fallback | `logFailure("sessions.spacesFoldedGet")` |
| components/sessions/space-row-menu.tsx:commitRename | inline error | `logFailure("sessions.fileRename")` |
| components/sessions/space-row-menu.tsx:confirmDelete | inline error | `logFailure("sessions.fileDelete")` |
| components/sessions/space-row-menu.tsx:withPath | inline error | `logFailure("sessions.filePath")` |
| components/settings/about-section.tsx:AboutSection debug read | fallback | `logFailure("about.debugModeGet")` |
| components/settings/about-section.tsx:AboutSection log path read | fallback | `logFailure("about.logPath")` |
| components/settings/about-section.tsx:AboutSection version read | fallback | `logFailure("about.version")` |
| components/settings/about-section.tsx:onAutoUpdateChange | fallback | `logFailure("about.autoUpdateSet")` |
| components/settings/about-section.tsx:onCheckForUpdates | inline error | `logFailure("update.check")` |
| components/settings/about-section.tsx:onDebugModeChange | fallback | `logFailure("about.debugModeSet")` |
| components/settings/about-section.tsx:onDownloadAndInstall | inline error | `logFailure("update.install")` |
| components/settings/about-section.tsx:onDownloadAndInstall relaunch | fallback | `logFailure("update.relaunch")` |
| components/settings/about-section.tsx:onRestart | inline error | `logFailure("update.restart")` |
| components/settings/about-section.tsx:refreshEgress | fallback | `logFailure("about.egress")` |
| components/settings/account-section.tsx:answerListening | inline error | `logFailure("account.answerListening")` |
| components/settings/account-section.tsx:Cancel sign-in button | inline error | `logFailure("account.cancelSignIn")` |
| components/settings/account-section.tsx:run | inline error | `logFailure("account.action")` |
| components/settings/agents-mcp-section.tsx:McpServerForm save | inline error | `logFailure("agents.mcpSave")` |
| components/settings/agents-mcp-section.tsx:reload | inline error | `logFailure("agents.mcpList")` |
| components/settings/agents-mcp-section.tsx:remove | inline error | `logFailure("agents.mcpRemove")` |
| components/settings/agents-mcp-section.tsx:SandboxSection poll | inline error | `logFailure("agents.sandboxReread")` |
| components/settings/agents-mcp-section.tsx:SandboxSection read | inline error | `logFailure("agents.sandboxGet")` |
| components/settings/agents-mcp-section.tsx:SandboxSection save | inline error | `logFailure("agents.sandboxSave")` |
| components/settings/agents-section.tsx:AgentSignIn submit | inline error | `logFailure("agents.copySignIn")` |
| components/settings/agents-section.tsx:AgentsSection MCP/sandbox read | fallback | `logFailure("agents.macKeeps")` |
| components/settings/agents-section.tsx:preview | inline error | `logFailure("agents.seedPlan")` |
| components/settings/agents-section.tsx:reload | inline error | `logFailure("agents.copies")` |
| components/settings/agents-section.tsx:reloadOffer | inline error | `logFailure("agents.seedOffer")` |
| components/settings/agents-section.tsx:repin | inline error | `logFailure("agents.driveRepin")` |
| components/settings/agents-section.tsx:write | inline error | `logFailure("agents.seedApply")` |
| components/settings/at-rest-encryption-choice.tsx:handleContinue | inline error | `logFailure("encryption.postureSet")` |
| components/settings/bot-grants-section.tsx:revoke | inline error | `logFailure("bots.grantRevoke")` |
| components/settings/bots-section.tsx:addOffer | inline error | `logFailure("bots.offerAddProvider")` |
| components/settings/bots-section.tsx:BotForm save | inline error | `logFailure("bots.botSave")` |
| components/settings/bots-section.tsx:probeBot | inline error | `logFailure("bots.botProbe")` |
| components/settings/bots-section.tsx:probeProvider | inline error | `logFailure("bots.providerProbe")` |
| components/settings/bots-section.tsx:ProviderForm save | inline error | `logFailure("bots.providerSave")` |
| components/settings/bots-section.tsx:ProviderForm save credential source | inline error | `logFailure("bots.providerCredentialSource")` |
| components/settings/bots-section.tsx:remove | inline error | `logFailure("bots.remove")` |
| components/settings/bots-section.tsx:VoiceEvents read | fallback | `logFailure("voice.events")` |
| components/settings/device-verification-dialog.tsx:VerificationBody start | inline error | `logFailure("verification.start")` |
| components/settings/device-verification-dialog.tsx:WaitingOrReady Verify with emoji | inline error | `logFailure("verification.startSas")` |
| components/settings/key-backup-dialog.tsx:CreateBackup enable | inline error | `logFailure("keyBackup.enable")` |
| components/settings/key-backup-dialog.tsx:restore | inline error | `logFailure("keyBackup.restore")` |
| components/settings/key-backup-dialog.tsx:Save to Keychain | inline error | `logFailure("keyBackup.saveRecoveryKey")` |
| components/settings/no-background-sync-disclosure.tsx:NoBackgroundSyncDisclosure read | fallback | `logFailure("sync.iosDisclosureShownGet")` |
| components/settings/phone-push.tsx:confirm | inline error | `logFailure("push.unifiedpushConfirm")` |
| components/settings/phone-push.tsx:PhonePush read | fallback | `logFailure("push.unifiedpushStatus")` |
| components/settings/push-gateway-field.tsx:PushGatewayField read | inline error | `logFailure("push.gatewayGet")` |
| components/settings/push-gateway-field.tsx:save | inline error | `logFailure("push.gatewaySet")` |
| components/settings/settings-dialog.tsx:AppSection badge read | fallback | `logFailure("settings.dockBadgeModeGet")` |
| components/settings/settings-dialog.tsx:AppSection launch read | fallback | `logFailure("settings.launchAtLoginGet")` |
| components/settings/settings-dialog.tsx:AppSection menu bar read | fallback | `logFailure("settings.menuBarGet")` |
| components/settings/settings-dialog.tsx:HotkeyRow assign | inline error | `logFailure("settings.hotkeySet")` |
| components/settings/settings-dialog.tsx:NotificationsSection badge read | fallback | `logFailure("settings.dockBadgeModeGet")` |
| components/settings/settings-dialog.tsx:NotificationsSection onBadgeModeChange | fallback | `logFailure("settings.dockBadgeModeSet")` |
| components/settings/settings-dialog.tsx:NotificationsSection onCheckedChange | fallback | `logFailure("settings.previewEnabledSet")` |
| components/settings/settings-dialog.tsx:NotificationsSection permission read | fallback | `logFailure("settings.notificationPermission")` |
| components/settings/settings-dialog.tsx:NotificationsSection preview read | fallback | `logFailure("settings.previewEnabledGet")` |
| components/settings/settings-dialog.tsx:onLaunchChange | fallback | `logFailure("settings.launchAtLoginSet")` |
| components/settings/settings-dialog.tsx:onMenuBarChange | fallback | `logFailure("settings.menuBarSet")` |
| components/settings/settings-dialog.tsx:onModeChange | fallback | `logFailure("settings.dockBadgeModeSet")` |
| components/settings/settings-dialog.tsx:onWindowChange | fallback | `logFailure("settings.undoSendWindowSet")` |
| components/settings/settings-dialog.tsx:PrivacySection incognito read | fallback | `logFailure("settings.incognitoGetGlobal")` |
| components/settings/settings-dialog.tsx:PrivacySection onCheckedChange | fallback | `logFailure("settings.incognitoSetGlobal")` |
| components/settings/settings-dialog.tsx:PrivacySection undo window read | fallback | `logFailure("settings.undoSendWindowGet")` |
| components/settings/settings-dialog.tsx:RemoteDeletionsToggle onCheckedChange | fallback | `logFailure("settings.honorRemoteDeletionsSet")` |
| components/settings/settings-dialog.tsx:RemoteDeletionsToggle read | fallback | `logFailure("settings.honorRemoteDeletionsGet")` |
| components/settings/settings-dialog.tsx:SecuritySection posture read | fallback | `logFailure("settings.encryptionPosture")` |
| components/settings/sync-git-row.tsx:commit | inline error | `logFailure("syncGit.pathSet")` |
| components/settings/sync-git-row.tsx:load | inline error | `logFailure("syncGit.status")` |
| components/settings/sync-section.tsx:DeviceRow read | inline error | `logFailure("sync.device")` |
| components/settings/sync-section.tsx:DriveFolderRow choose | stuck state | `logFailure("sync.pickDriveFolder")` |
| components/settings/sync-section.tsx:DriveFolderRow write | inline error | `logFailure("sync.driveFolderSet")` |
| components/settings/sync-section.tsx:FolderPathLink | inline error | `logFailure("sync.openPath")` |
| components/settings/sync-section.tsx:ProfileRow run | inline error | `logFailure("sync.action")` |
| components/settings/sync-section.tsx:renameDevice | inline error | `logFailure("sync.deviceSetLabel")` |
| components/settings/sync-section.tsx:saveLists | inline error | `logFailure("sync.listSettingsSet")` |
| components/settings/sync-section.tsx:SyncSection lists read | inline error | `logFailure("sync.listSettingsGet")` |
| components/settings/tasks-section.tsx:choose | inline error | `logFailure("tasks.ledgerSet")` |
| components/settings/tasks-section.tsx:reload | inline error | `logFailure("tasks.ledgerRead")` |
| components/settings/telemetry-section.tsx:change | inline error | `logFailure("telemetry.consentSet")` |
| components/settings/telemetry-section.tsx:refresh | inline error | `logFailure("telemetry.status")` |
| components/settings/transcription-section.tsx:Fetch models button | inline error | `logFailure("transcription.modelsFetch")` |
| components/settings/transcription-section.tsx:pick | inline error | `logFailure("transcription.pick")` |
| components/settings/transcription-section.tsx:TranscriptDialog onClose | inline error | `logFailure("transcription.voicesDriveRefresh")` |
| components/settings/transcription-section.tsx:TranscriptionSection models read | inline error | `logFailure("transcription.modelsAvailable")` |
| components/settings/transcription-section.tsx:VoicesDrive read | inline error | `logFailure("transcription.voicesDrive")` |
| components/settings/transcription-section.tsx:VoicesDrive run | inline error | `logFailure("transcription.voicesEdit")` |
| components/sync/add-folder-form.tsx:AddFolderForm credential read | inline error | `logFailure("addFolder.credentialRead")` |
| components/sync/add-folder-form.tsx:submit | inline error | `logFailure("addFolder.save")` |
| components/sync/add-folder-form.tsx:submit credential source | inline error | `logFailure("addFolder.credentialSource")` |
| components/sync/add-folder-form.tsx:submit keychain leg | inline error | `logFailure("addFolder.credentialWrite")` |
| components/sync/browse-repos-sheet.tsx:AddRepos run | inline error | `logFailure("browseRepos.add")` |
| components/sync/browse-repos-sheet.tsx:ConnectPanel connect start | inline error | `logFailure("browseRepos.connectStart")` |
| components/sync/browse-repos-sheet.tsx:ConnectPanel connect wait | inline error | `logFailure("browseRepos.connectWait")` |
| components/sync/browse-repos-sheet.tsx:ConnectPanel GitHub apps link | inline error | `logFailure("browseRepos.openApps")` |
| components/sync/browse-repos-sheet.tsx:ConnectPanel open host | inline error | `logFailure("browseRepos.connectOpen")` |
| components/sync/browse-repos-sheet.tsx:Disconnect button | inline error | `logFailure("browseRepos.disconnect")` |
| components/sync/browse-repos-sheet.tsx:RepoList load | inline error | `logFailure("browseRepos.listing")` |
| components/sync/browse-repos-sheet.tsx:SourceProblem act | inline error | `logFailure("browseRepos.fixSource")` |
| components/sync/task-form.tsx:submit | inline error | `logFailure("taskForm.save")` |
| components/sync/task-form.tsx:TaskForm profiles read | inline error | `logFailure("taskForm.profiles")` |
| components/sync/task-form.tsx:TaskForm schedule preview | not IPC | left: a refusal is the cron dialect rejecting a half-typed expression on each keystroke (a validation answer, not a failed call); the save reports real failures |
| components/transcription/copy-clip.tsx:CopyClip compose | inline error | `logFailure("transcript.clipCompose")` |
| components/transcription/copy-clip.tsx:CopyClip submit | inline error | `logFailure("transcript.clipCopy")` |
| components/transcription/transcript-player.tsx:start | inline error | `logFailure("transcript.play")` |
| components/transcription/transcript-viewer.tsx:act | inline error | `logFailure("transcript.edit")` |
| components/transcription/transcript-viewer.tsx:TranscriptViewer media | inline error | `logFailure("transcript.media")` |
| components/transcription/transcript-viewer.tsx:TranscriptViewer read | inline error | `logFailure("transcript.read")` |
| components/transcription/transcription-job.tsx:TranscriptionJob cancel | inline error | `logFailure("transcription.cancel")` |
| components/viewers/markdown-preview.ts:mountMarkdownPreview chunk load | fallback | `logFailure("preview.editorLoad")` |
| components/viewers/text-editor-host.ts:mountTextEditor grammar | fallback | `logFailure("editor.grammarLoad")` |
| components/viewers/text-file-frame.tsx:TextFileFrame PDF button | inline error | `logFailure("viewer.exportPdf")` |
| components/viewers/use-document-file.ts:useDocumentFile | inline error | `logFailure("viewer.readDocument")` |
| components/viewers/use-text-file.ts:read | inline error | `logFailure("textFile.read")` |
| components/viewers/use-text-file.ts:save | inline error | `logFailure("textFile.save")` |
| hooks/use-auto-update.ts:attempt check | inline error | `logFailure("update.check")` |
| hooks/use-auto-update.ts:attempt install | inline error | `logFailure("update.install")` |
| hooks/use-auto-update.ts:restartCycle | fallback | `logFailure("update.restartCheck")` |
| hooks/use-bbctl-run.ts:useBbctlRun start | inline error | `logFailure("bbctl.runStart")` |
| hooks/use-bridge-catalog.ts:useBridgeCatalog | inline error | `logFailure("bridges.catalog")` |
| hooks/use-bridge-discovery.ts:useBridgeDiscovery | inline error | `logFailure("bridges.discover")` |
| hooks/use-bridge-login.ts:useBridgeLogin start | inline error | `logFailure("bridgeLogin.start")` |
| hooks/use-capabilities-hydrate.ts:hydrate | fallback | `logFailure("capabilities.hydrate")` |
| hooks/use-capabilities-hydrate.ts:listenCapabilitiesChanged | fallback | `logFailure("capabilities.listen")` |
| hooks/use-capture-draft.ts:dismiss | inline error | `logFailure("capture.hide")` |
| hooks/use-capture-draft.ts:resolve | inline error | `logFailure("capture.draft")` |
| hooks/use-coupling-caveats.ts:useCouplingCaveats | fallback | `logFailure("bridges.couplingCaveats")` |
| hooks/use-notes-body.ts:notesOpen | inline error | `logFailure("notes.open")` |
| hooks/use-notes-changes.ts:load | inline error | `logFailure("notes.listWindow")` |
| hooks/use-notes-search.ts:useNotesSearch | inline error | `logFailure("notes.search")` |
| hooks/use-recorded-session-summary.ts:useRecordedSessionSummary | fallback | `logFailure("recording.sessionSummary")` |
| hooks/use-recording-session.ts:start | inline error | `logFailure("recording.start")` |
| hooks/use-sessions-changes.ts:readRows | inline error | `logFailure("sessions.list")` |
| hooks/use-sessions-search.ts:useSessionsSearch | inline error | `logFailure("sessions.search")` |
| lib/agents/surface.ts:reportView | fallback | `logFailure("agents.presenceView")` |
| lib/export/export-target.ts:exportTarget | inline error | `logFailure("export.target")` |
| lib/ipc/client.ts:botsImagePaste | translate | left: envelope translation, rethrown to the caller (bot-attachment.tsx:handle reports bots.imagePaste) |
| lib/ipc/client.ts:invoke | translate | left: envelope translation: rethrows an IpcError (or normalises a stray rejection into one) to the caller, which reports it |
| lib/ipc/client.ts:sendAttachmentBytes | translate | left: envelope translation, rethrown to the caller |
| lib/notes/follow-link.ts:followExternalUrl | inline error | `logFailure("notes.openExternal")` |
| lib/notes/follow-link.ts:resolveWikilink | inline error | `logFailure("notes.resolveLink")` |
| lib/recording-control.ts:startRecordingWithCurrentSelections | stuck state | `logFailure("recording.start")` |
| lib/recording-control.ts:stopRecording | stuck state | `logFailure("recording.stop")` |
| lib/recording-control.ts:toggleRecording | stuck state | `logFailure("recording.status")` |
| lib/recordings-space.ts:useRecordingsSpace | fallback | `logFailure("notes.recordingsSpace")` |
| lib/stores/config-layers.ts:refreshConfigLayers | inline error | `logFailure("config.layers")` |
| lib/stores/copy-job.ts:cancelCopyJob | inline error | `logFailure("copy.cancel")` |
| lib/stores/copy-job.ts:refreshCopyJob | inline error | `logFailure("copy.status")` |
| lib/stores/copy-job.ts:startCopyJob | inline error | `logFailure("copy.start")` |
| lib/stores/notes-filters.ts:persistHideServiceFiles | translate | left: reverts the switch and rethrows; the write queue logs notes.persistHideServiceFiles and the caller shows the sentence |
| lib/stores/notes-filters.ts:persistIncludePrivate | translate | left: reverts the switch and rethrows; the write queue logs notes.persistIncludePrivate and note-filter-bar reports notes.includePrivateSet |
| lib/stores/notes-vaults.ts:ensureNotesVaultsHydrated | inline error | `logFailure("notes.vaultsHydrate")` |
| lib/stores/notes-vaults.ts:refreshNoteVaults | inline error | `logFailure("notes.vaultsRefresh")` |
| lib/stores/notes-vaults.ts:setActiveVault | inline error | `logFailure("notes.vaultSetActive")` |
| lib/stores/outbox.ts:undoHeldSend | stuck state | `logFailure("outbox.cancelHeldSend")` |
| lib/stores/recording-capture-sources.ts:ensureCaptureSourcesHydrated | fallback | `logFailure("recording.captureSourcesGet")` |
| lib/stores/recording-settings.ts:applyRecordingSettings | inline error | `logFailure("recording.settingsSet")` |
| lib/stores/recording-settings.ts:ensureRecordingSettingsHydrated | stuck state | `logFailure("recording.settingsGet")` |
| lib/stores/sessions-roots.ts:refreshSessionsRoots | inline error | `logFailure("sessions.roots")` |
| lib/stores/sync-detail.ts:retrySyncParkedAll | translate | left: keeps the first rejection and rethrows it after the loop; the caller (sync-pane run) reports it as syncPane.action |
| lib/stores/sync.ts:ensureSyncHydrated | inline error | `logFailure("sync.hydrate")` |
| lib/stores/sync.ts:refreshSyncProfiles | inline error | `logFailure("sync.profiles")` |
| lib/stores/sync.ts:refreshSyncStatuses | inline error | `logFailure("sync.statuses")` |
| lib/stores/transcription.ts:refreshTranscription | inline error | `logFailure("transcription.status")` |
| lib/stores/transcription.ts:saveTranscriptionSettings | inline error | `logFailure("transcription.settingsSet")` |
| lib/stores/transcription.ts:startTranscription | inline error | `logFailure("transcription.start")` |
| lib/titlebar-drag.ts:beginTitleBarDrag | caller reports | left: report("refused", …) already writes the refusal to keeper.log through titlebar_drag_report (its own failure logs titlebar.report) |
| lib/vault-link/actions.ts:openNoteForFile | inline error | `logFailure("notes.openNoteForFile")` |
| print-main.tsx:PrintDocument read | inline error | `logFailure("print.readText")` |
| study/failure.ts:logStudyFailure | not IPC | left: the funnel itself: keeper.log refused the line, so the console is the only witness (another report would loop) |
| study/session.ts:startStudyCapture stop | translate | left: rethrows a redacted Error to its callers, which report it (logStudyFailure("study.stop") in abort/fail, and study.tsx:stop) |
| study/study.tsx:start | inline error | `logStudyFailure("study.start")` |
| study/study.tsx:stop | inline error | `logStudyFailure("study.stop")` |
| study/study.tsx:Study registerClose | inline error | `logStudyFailure("study.registerClose")` |
| study/transport.ts:installStudyTransport fetch | translate | left: `throw refuse()` hands a redacted refusal to the SDK, whose failure callback ends the study with a notice |
| voice-main.tsx:VoiceApp listenVoiceState | not IPC | left: the voice window's capability grants no command (frontend_error_report included); the console is the only witness |

## Fix round 2 (R410, review-100errors-2 R100E2-02/03)

The two recording-settings rows above were counted complete on the store's catch alone; neither reached the person. Corrected dispositions, with the consumers checked:

| Site | Was | Now |
| --- | --- | --- |
| lib/stores/recording-settings.ts:ensureRecordingSettingsHydrated | stuck state, logged only | logged **and** `readError` = `RECORDING_SETTINGS_UNREAD` while no VM was ever read (cleared by a later successful read). `RecordingSettingsControls` renders the sentence (`role="alert"`) in place of the blank fields — it is on both settings surfaces (Settings › Recording, the pane's Segmenting card), so each says it once; the Destination and Advanced cards stay disabled beside it. The note recorder (`MediaRecorderPanel`), whose echo switch is the only settings control, says it beside Start. |
| lib/stores/recording-settings.ts:applyRecordingSettings | inline error assumed | the refusal string is consumed by every caller: `useRecordingSettingsWriter` (latest write's answer; a stale one never overwrites it) prints it under the segment/duration fields, the Advanced pickers and the echo-cancellation switch; `RecordingDestinationControls` already printed it for the template and destination. No `void applyRecordingSettings(...)` call is left. |
| components/chat/composer.tsx:flushDraft (empty body) | `clearDraft` logged only; the draft-save failure line outlived a manual delete to empty | the emptied draft's deletion is the chat's latest write: when it lands the line goes; when it fails it is logged and the line stays (older words may still be stored). Writes are sequenced per chat, so a stale save/deletion neither clears nor sets a newer write's line; `clearPersistedDraft` (send) supersedes writes in flight. |

