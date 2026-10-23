# keeper on Android

keeper builds for Android from the same Tauri project as the Mac and the iPhone
(Epic 98, story 98.3; AD-414). The target the owner asked for is an Android
tablet; a phone runs the same build. It is a client, like keeper on the iPhone:
it reads and writes your Matrix rooms and the drive folders it syncs, it never
runs an agent, and it is installed by sideloading a signed APK — there is no
Play Store or F-Droid build (DW-407).

**What is proved where.** Nothing Android runs on keeper's Linux dev host and
no Android device has run keeper yet:

- **CI's `Android (compile and debug APK)` job** (`.github/workflows/ci.yml`,
  `ubuntu-latest`) is the gate: it type-checks `keeper`, `keeper-core`,
  `keeper-sync` and `keeper-agent` for `aarch64-linux-android` with the NDK,
  builds a debug APK (signed with the runner's throwaway debug key, as every
  debug build is — not an installable release), scans what Gradle resolved
  for it (below) and uploads the APK as an artifact. **Its first green run is
  owed on this rung's pull request**; until then the Android half has been
  type-checked locally only, and neither Gradle nor `tauri android build` has
  run anywhere. The macOS job and the iOS compile check must stay green on the
  same change.
- **On Linux:** the secret file's format (`keeper_core::secret_file`, the
  `android_secret_store` tests), the Android dependency graph
  (`bun run check:android-graph`) and the Gradle licence scan's rules and
  tables (`android_gradle_licences`, below).
- **On the tablet:** the device run in story 98.3 #7, recorded in
  `docs/agents.md` § *Measured* — owed until it is done.

## Prerequisites

On the machine that builds (not this repository's Linux container, which has no
JDK, SDK or NDK):

- A JDK 17 (`JAVA_HOME`).
- The Android SDK (`ANDROID_HOME`) with `platforms;android-36`, a recent
  `build-tools` (for `zipalign` and `apksigner`) and `platform-tools` (`adb`).
- The Android NDK (`NDK_HOME`), r27 or newer.
- The Rust target: `rustup target add aarch64-linux-android` (add
  `armv7-linux-androideabi`, `i686-linux-android` and `x86_64-linux-android` for
  an emulator or an older device).
- `bun install` at the repository root.

## The Android project

`src-tauri/crates/keeper/gen/android/` is the Android Studio project, generated
once by `tauri android init` (tauri-cli 2.11.4) and committed, like
`gen/apple/`. What keeper changed in it after generation:

- `app/src/main/java/dev/tgorka/keeper/MainActivity.kt` writes three facts
  into the process environment **before** `super.onCreate`, because keeper's
  Rust half starts inside it and has no other way to ask Android that early:
  `KEEPER_ANDROID_FILES_DIR` (the app's private files directory),
  `KEEPER_ANDROID_DEVICE_CLASS` (`tablet` at a smallest width of 600 dp, else
  `mobile`) and `KEEPER_ANDROID_MODEL`. None is a secret.
- `app/src/main/java/dev/tgorka/keeper/KeystorePlugin.kt` — the Keystore half
  of the secret store (below).
- `UnifiedPushPlugin.kt`, `PushBridge.kt`, `PushReceiver.kt` and
  `PushActionReceiver.kt` in the same package — push through your UnifiedPush
  distributor ([Push](#push) below), with the two receivers declared in
  `AndroidManifest.xml`.
- `AndroidManifest.xml`: no backups (`allowBackup="false"`,
  `fullBackupContent="false"` and `res/xml/data_extraction_rules.xml`, which
  exclude every domain from cloud backup and device transfer) and the
  `keeper://` / `dev.tgorka.keeper:` deep link, in the block the deep-link
  plugin's build script rewrites from `tauri.conf.json` on every Android build.
- `app/build.gradle.kts` declares no test dependencies: the project has no
  Android tests to run them.

**Permissions.** The committed manifest asks for `INTERNET` only; Gradle merges
the libraries' manifests into the APK's. The merged release manifest (read from
a Gradle `processUniversalReleaseMainManifest` run on 2026-10-12) asks for:

- `INTERNET` — needed: Matrix, sync, sign-in.
- `POST_NOTIFICATIONS` (the notification plugin) — needed: keeper's
  notifications. On Android 13 and later it is a runtime permission, which
  keeper asks for at start whenever it is not granted yet (the same startup
  thread that asks on every other target).
- `RECEIVE_BOOT_COMPLETED` and `WAKE_LOCK` (the notification plugin) — idle:
  the plugin's `LocalNotificationRestoreReceiver` (`BOOT_COMPLETED`,
  `LOCKED_BOOT_COMPLETED`, `QUICKBOOT_POWERON`, not exported) puts back
  *scheduled* local notifications after a reboot, and keeper schedules none.
  Android starts keeper's process to deliver the broadcast, the receiver finds
  nothing to restore, and keeper's own code — which starts with its activity —
  does not run. Its `TimedNotificationPublisher` (fires a scheduled
  notification) is idle for the same reason; `NotificationDismissReceiver`
  hears a notification being swiped away. Neither is exported.
- `dev.tgorka.keeper.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION` (AndroidX Core)
  — a `signature` permission keeper defines and holds itself, so receivers
  AndroidX registers at run time take no broadcast from another app.

The UnifiedPush connector (98.4) adds `WAKE_LOCK`, already there, and a
`<queries>` block naming the distributor's `REGISTER` and `UNREGISTER` actions,
so keeper can see which distributors are installed and read their names (read
from the connector 2.5.0 AAR's own manifest; no merged manifest of story 98.4's
tree was kept, so the permission list above is expected, not observed, to be
unchanged by it — CI's `android` job is the first build that shows it).
keeper's `PushReceiver` is exported, as the connector requires: every
broadcast to it carries the registration's random token, which the connector
checks before keeper sees anything. `PushActionReceiver` is not exported.

Also merged in, from AndroidX: `androidx.startup`'s `InitializationProvider`
(not exported) and `profileinstaller`'s `ProfileInstallReceiver`, which is
exported but guarded by `android.permission.DUMP`, so only `adb` and the
system can send to it. No location, contacts, storage, camera or microphone
permission is in the build.

`src-tauri/crates/keeper/tauri.android.conf.json` is merged over
`tauri.conf.json` for Android builds: no `externalBin` (no sidecar runs on
Android) and `minSdkVersion` 24, the notification plugin's floor.

To regenerate after a Tauri upgrade, run `bun run tauri android init` from the
repository root on a machine with the SDK and NDK, then re-apply the changes
above; `git diff` shows them.

## Build

```sh
bun run android:dev      # a debug build on a connected device or emulator
bun run android:build    # release APKs (unsigned) for every Android target
```

`tauri android build` drives Gradle, whose `rust` plugin calls back into the
Tauri CLI to build `libkeeper_lib.so` for each ABI. The unsigned release APK
lands under `gen/android/app/build/outputs/apk/`.

## Sign and sideload

The signing keystore is the owner's and never enters this repository. Create
it once, on the machine that builds:

```sh
keytool -genkeypair -v -keystore ~/keeper-release.jks -alias keeper \
  -keyalg RSA -keysize 4096 -validity 10000
```

and export, for the session that installs:

| Variable | What |
|---|---|
| `KEEPER_ANDROID_KEYSTORE` | the keystore file |
| `KEEPER_ANDROID_KEYSTORE_PASSWORD` | its password — `apksigner` reads it from the environment, never from a command line |
| `KEEPER_ANDROID_KEY_ALIAS` | the key's alias (`keeper` above) |

On the device: Developer options → USB debugging (or *Install unknown apps*
for the file manager you copy the APK with). Then:

```sh
bun run install:android                 # build, zipalign, sign, verify, adb install -r
bun run install:android -- --no-install # build and sign only; prints the APK's path
bun run install:android -- --serial <id>
```

Every later install must be signed with the same key: Android refuses an update
signed by another one, and the only way out is uninstalling, which removes
keeper's data on the device.

## Sign-in

- **An organisation account or a Matrix single sign-on** opens its
  authorization page in an **Auth Tab** (`AuthTabPlugin.kt`, androidx.browser
  1.10; `web_auth.rs`): the browser's cookies, password manager and passkeys,
  in a tab stripped down to signing in. When the page redirects to keeper's
  callback (`keeper://oauth/…` or `dev.tgorka.keeper:/oauth/callback`) the
  browser captures that navigation and hands the URL back to keeper as the
  tab's result — it never travels as an intent, so no other app that declares
  the same scheme can claim it. Closing the tab cancels that one sign-in; a
  browser that cannot verify the address ends it with the reason.
- **Which browser.** The one Android opens Custom Tabs in — your default browser
  when it supports them — and only when that browser supports Auth Tab. keeper
  sends the tab to that browser by name, so the tab that opens is the one it
  checked. The manifest declares the browsers it looks for (`<queries>`), which
  Android 11 and later require. Another installed browser with Auth Tab is not
  sought yet (DW-1304).
- **When that browser has no Auth Tab** the page opens wherever Android sends it — a
  Custom Tab or the browser itself — and the callback returns through the
  `keeper://` / `dev.tgorka.keeper:` deep link the manifest declares, as from
  any browser. That path is the one another app could intercept; the
  authorization code it carries is useless without the PKCE verifier, which
  never leaves keeper's process. Android often reports such a tab as ended the
  moment it opens, so its ending ends nothing: the sign-in finishes on the deep
  link, on your own cancel, or after five minutes.
- **Why not a verified `https` App Link.** Auth Tab can return on an `https`
  address the browser verifies against Digital Asset Links, which would close
  the fallback's gap too. It needs three things keeper cannot supply from this
  repository: a `/.well-known/assetlinks.json` on the redirect's host
  (`keeper.tgorka.dev`, keeper's `client_uri`) naming the SHA-256 of **each
  signer's own key** — every sideloaded build is signed by its owner; Matrix
  homeservers that admit a native client by MSC2966 to accept an `https`
  redirect from it, which they refuse for `application_type: native` (only a
  reverse-DNS scheme or a loopback address); and every organisation's identity
  provider registering the new address. DW-1300 holds it.
- **Links** ("Open in browser") open in a Custom Tab through the opener plugin.
  The plugin's free `open_url` is the `open` crate, which on Android spawns a
  desktop opener that does not exist, so keeper never calls it there.

## Push

Your agents' approval requests and your own agent's answers can reach this
device while keeper is in the background — through a UnifiedPush
**distributor** you install and a server you run (story 98.4; AD-414, D-36).
keeper ships no push server and never picks one (D-1, D-4).

**What you set up (operator actions).**

- A UnifiedPush server that is also a Matrix push gateway, reachable over
  `https://` by your homeserver and by this device — ntfy (Apache-2.0) is one:
  it answers the Matrix push path on its own address. keeper registers push
  only over `https://`.
- The distributor app for it on this device — the ntfy app, pointed at that
  server.

**What keeper does.**

1. At start keeper asks Android which distributors are installed, by package
   and by the name Android shows. keeper registers with a distributor only
   after **you confirmed that one** on this device: Settings → Notifications
   names each installed distributor — name and package, because any app can
   call itself a distributor and a name alone can be borrowed — and asks; the
   only one installed is asked about too. Nothing registers before you answer.
   Your choice is kept in keeper's data directory
   (`unifiedpush-distributor`), on this device only — nothing syncs it; while
   that distributor stays installed keeper uses it without asking, and if it
   is gone, a different one is asked about again
   (`unifiedpush::choose_distributor`). keeper never chooses for you.
2. The distributor gives keeper an **endpoint**: an address on its server that
   delivers whatever is posted to it to keeper here. Its path is a secret —
   anyone who has it can post to this device — so keeper never logs or shows it,
   only its host; it is kept in keeper's data directory as
   `unifiedpush-endpoint`, with the distributor it came from and whether its
   server answered as a gateway, so a sign-out can remove the pusher.
3. keeper asks the endpoint's own server once per endpoint whether it is a
   Matrix push gateway: `GET https://<endpoint host>/_matrix/push/v1/notify`,
   which a gateway answers with `{"unifiedpush":{"gateway":"matrix"}}`
   (`unifiedpush::discover_gateway`). An endpoint it kept whose server answered
   so is not asked again at the next start (`unifiedpush::known_gateway`); a
   new endpoint is. This is the one request keeper itself makes to the push
   server — on the iPhone it contacts its gateway never. A redirect is not
   followed. A definite other answer — a 404, something that is not that JSON,
   a UnifiedPush gateway of another kind — registers nothing (and removes a
   pusher registered before), and Settings → Notifications says why, naming
   the host. **No answer** — the server unreachable, or slower than 10
   seconds — is not an answer: keeper changes nothing, a pusher registered
   before stays, and it asks again at the next start
   (`unifiedpush::pusher_change`). No other gateway is ever tried.
4. Before 108.2 keeper registered on every account that held agent session
   rooms an `event_id_only` pusher to that gateway (app
   `dev.tgorka.keeper.android`, pushkey the endpoint). The agents left Matrix,
   so each new endpoint takes this device's pusher for it — and for the
   endpoint before it — off every account and registers none
   (`AccountManager::push_remove`, DW-E108-1; kept for one release). A
   sign-out removes it from that account first (best effort).
5. A push of the agents' old Matrix path (an event id) shows nothing.
6. The notifications and their actions are the agents server's items' (7).
7. **From the agents server** (106.6): the same endpoint and gateway are this
   device's push target on the agents server (`PUT /clients/self/push-target`,
   removed when the endpoint or the gateway goes). Its push names an item by
   ids; keeper reads that item from the server with this device's pairing and
   posts what the iPhone would show, one notification per item, so a second
   delivery replaces the first. Its actions write the decision row through the
   session's replica; a tap opens the session in the Agents window
   ([agents.md](agents.md#a-push-from-the-agents-server)). This works **while
   keeper is running**; after Android has stopped keeper's process it does
   nothing more than step 8, and an action on a notification posted earlier
   cannot decide — deferred by the owner (R-HQ-58, DW-E106-15). Owed on the
   tablet.
8. Once Android has stopped keeper, a push cannot be read — keeper's accounts
   and keys start with its window — so keeper posts one notification of its own
   words ("Open keeper to see what arrived."), and each such push replaces the
   last.

**What is proved where.** On Linux: the pusher, the endpoint's grammar, the
discovery answer and request (against a local HTTPS server), asking once per
endpoint, an unreachable server keeping the pusher, what a push names, the
distributor confirmation and what a notification posts and offers
(`keeper-core/tests/unifiedpush_android.rs`,
`unifiedpush_gateway_discovery.rs`). The Kotlin compiled
(`compileUniversalDebugKotlin`) in a scratch copy of `gen/android` on Linux
before the confirmation was added; the confirmation's Kotlin (the
distributors' names), the notification intents' data URIs and the status-bar
icon are checked by inspection, and `push_android.rs` type-checked for
`aarch64-linux-android`; neither has been linked into an APK or run — CI's
`android` job is the first build. The device run — the pusher on tuwunel,
an approval from the lock screen, push latency — is story 98.4 #10, owed until
its row is in `docs/agents.md` § *Measured*. What is still open: push only
while keeper runs (DW-1310), registration only when the distributor answers at
start (DW-1311), no way to switch to another distributor while the confirmed
one stays installed (DW-1312, narrowed), no APK built yet (DW-1313), a tap's
room forgeable through the launcher activity (DW-1314), and no retry of an
unreachable server while keeper runs (DW-1315).

## Voice

Voice on Android is the iPhone's turn — the wake phrase, a question, the answer read
aloud, talking over it — with three differences, each a fact of the platform
(`keeper_core::voice::android`, AD-414, D-36):

1. **Only in front.** Android gives an app with no microphone foreground service
   silence once another app is in front or the screen is off, and keeper runs none.
   Listening stops when keeper leaves the front (the turn ends with "keeper stopped
   listening when it left the front…") and starts again, from the switch, when keeper is
   back — and after a relaunch, as on the iPhone and the Mac: a switch you left on is
   armed again once keeper has read the device, by the same rule (`should_rearm`,
   AD-190); keeper never turns it on for you (AD-169). Behind another app nothing is
   read aloud either: an answer that arrives there stays on the screen, and the turn
   ends with the same sentence.
2. **From Android 13.** keeper records the microphone itself — `AudioRecord` on
   `VOICE_COMMUNICATION`, with `AcousticEchoCanceler` on where the device has one —
   and feeds that audio to the on-device recogniser
   (`SpeechRecognizer.createOnDeviceSpeechRecognizer`) through `EXTRA_AUDIO_SOURCE`, in
   one segmented session whose results `keeper_core::voice::segments` joins into the
   utterance. `EXTRA_AUDIO_SOURCE` and segmented sessions arrived in Android 13; older
   Android is refused by name ("listening needs Android 13 or later…"). There is no
   server recogniser in keeper: a language this device has not installed for on-device
   recognition is refused with where to download it, never sent anywhere.
3. **Duplex is the device's.** With an echo canceller, the microphone stays open while
   keeper speaks and you can talk over the answer, as on the iPhone. Without one,
   keeper stops listening while it speaks — the stop word does nothing over an answer;
   the button still stops it. The one sentence beside the switch names both cases,
   because which one this device is is read after the sentence may be on screen.

The microphone is asked for at your first voice act (the switch or the microphone
button), never at launch; Android has no separate speech-recognition grant. Answers are
read by the text-to-speech engine in an installed voice that needs no network; a
language with none stays on the screen. The turn models of Epic 97 do not run here —
`ort` publishes an Android library, but this build links none (DW-413) — so a turn ends
when you pause, and Settings says *Turn models are not available on Android; keeper
waits 1.8 s after you stop*.

**What is proved where.** On Linux: the joining of segments, Android's availability,
grant, language and error rules, the platform's duplex and sentences, and the scan that
keeps every recogniser in the app on the device (`keeper-core`'s `voice::segments`,
`voice::android`, `tests/voice_platform.rs`, `tests/voice_on_device.rs`). The Rust port
type-checked for `aarch64-linux-android` and `VoicePlugin.kt` compiled in a scratch copy
of `gen/android` on Linux; nothing has linked or run them — CI's `android` job is the
first build. The device run — the permission dialog, a spoken question and its answer, talking
over it, whether the tablet has an echo canceller and whether recognition keeps working
while text-to-speech plays — is story 98.4 #10, owed until its row is in
`docs/agents.md` § *Measured*.

## Where keeper keeps its data

- **The data directory** is the app's private files directory
  (`Context.getFilesDir()`, typically `/data/user/0/dev.tgorka.keeper/files`):
  `keeper.db`, `archive.db`, `sync.db`, `accounts/<id>/sdk` and the phone's
  folders under `sync/`. Android removes it with the app. Without
  `KEEPER_ANDROID_FILES_DIR` (a `MainActivity` that lost its first lines) keeper
  logs that it has nowhere to keep its data rather than guessing a path.
- **Secrets** (`session/<id>`, `store_passphrase/<id>`,
  `sync/<profile>/credential`) are in `secrets.bin` in that directory: one
  XChaCha20-Poly1305 file — `KSEC`, a version byte, a random 24-byte nonce, the
  ciphertext of every key and value — rewritten whole and atomically on every
  change (`keeper_core::secret_file`). A flipped byte, another key or another
  version is refused, by reads and writes alike, and the file is never
  overwritten then: starting over is clearing keeper's storage in Android's
  settings.
- **Its key** is a random 32-byte data key that `KeystorePlugin` generates
  once, stores only wrapped (AES-256-GCM) by a key the Android Keystore
  generated and never lets out, in `noBackupFilesDir/keeper-secret-file.key`,
  and unwraps once per process for the Rust side. A wrapped key that no longer
  unwraps is refused, never replaced.
- **Never `keyring`.** Off Apple and desktop Linux, the `keyring` crate is an
  in-memory mock that forgets every secret at exit; `check:android-graph` fails
  if it is in the Android build.
- **No backups.** The manifest excludes everything; a restored copy would be
  bytes nobody could open anyway, because no backup carries a Keystore key.

## Limitations

These are the lines keeper shows in-app under **Settings → About → "On this
tablet"** (on an Android phone the same lines say "phone"):

- No agent runs on this tablet: your agents run on the agents server you run, and keeper here talks to them.
- A folder you add here mirrors a remote your Mac already syncs and lives inside keeper's own storage: it syncs only when keeper opens, comes back in front or you pull to refresh, and nothing is merged on this tablet — the Mac merges.
- The sessions board is here: read a session's cards and move one with Move to…, and an agent's approvals are decided in Agents; creating and archiving sessions stays on your Mac.
- No screen recording and no transcription on this tablet.
- keeper syncs and notifies about messages only while it is open. With a UnifiedPush distributor whose server is also a Matrix push gateway, your agents' approval requests and your own agent's answers reach this tablet while keeper runs in the background, read on this tablet (nothing else shows, and only ids pass through that server, never words); once Android has stopped keeper, a push only says that something arrived.
- Voice works from Android 13: speech is recognised on this tablet and never sent to a server, and the answer is read aloud. There are no turn models here, so keeper waits for a pause after you stop. Turn listening on while keeper is in front and it listens while keeper stays in front with the screen on. Switching to another app or turning the screen off stops it, and it starts again whenever keeper is back in front, after a relaunch too. Where this device has no echo canceller, keeper stops listening while it speaks, so saying the stop word over an answer does nothing and the button stops it. It stops when you turn it off. Android shows its microphone indicator the whole time, and listening uses battery.
- Updates arrive by installing a new build, signed with your own key and sideloaded; Check for updates in About says when a new version is out.

> This list is composed in Rust (`keeper_core::disclosure::android`) and shown by
> the About screen exactly as Rust sends it; `keeper-core/tests/platform_disclosure_mirrors_docs.rs`
> reads this list from disk and fails when the two differ. Edit both together or
> neither. The sixth line is `keeper_core::voice::android::disclosure_line`; § Voice
> says the same at length.

And, for whoever builds it:

- Sync on Android runs gitoxide in-process, like the iPhone
  (`GitEngine::HOST`): there is no `git` binary to spawn.
- Media responses are capped at 8 MiB per range request, as on the iPhone,
  so one large video cannot allocate a second full copy in memory.
- keeper's own URLs (`keeper-media`, `keeper-file`, `keeper-note`,
  `keeper-recording`) are spelled `http://<scheme>.localhost/<host>/<path>`:
  Android's WebView cannot load a custom scheme, so Tauri serves each protocol
  on that host. The spelling is one Rust fact (`keeper_core::asset_url::UrlForm::HOST`,
  Tauri's own rule: Android and Windows), composed by `asset_url::compose` in
  Rust and `src/lib/media-src.ts` in the front, which reads it from the facts
  the shell writes into the webview before the bundle runs; every parser takes
  either spelling, because the handler cannot choose which one the webview
  sends and one parser serves every build.
- The layout: a tablet — by Android's own rule, the device's smallest width at
  least 600 dp, which `MainActivity` reads and Rust serves as
  `PlatformVm.tablet` — shows the phone's surfaces with the proxy dock as a
  sheet from the Inbox header; at 840 dp wide and above, the rooms list and the
  open room side by side (`use-shell-layout.ts`, UX-DR146). The viewport never
  decides it, so the keyboard opening keeps the dock and the rooms list.
- A PDF in Files or in a note has no viewer here: Android's WebView draws no
  `<embed type="application/pdf">` (DW-1303).

## What changed in the shell for Android (98.3)

Every `cfg(target_os = "ios")` site was reviewed; this is the list and the
decision for each.

| Site | Decision |
|---|---|
| `keeper-sync` `GitEngine::HOST` | **widened**: gitoxide on Android too |
| `ipc.rs` `AppState::new` guard | **narrowed**: fails only on a mobile target that is neither iOS nor Android |
| `ipc.rs` / `recorder.rs` recorder | **widened**: `MobileRecorder` (`cfg(mobile)`), honestly `Unsupported` |
| `media_protocol.rs` range cap | **widened**: 8 MiB on Android too |
| `keeper-core` `agents::mac_tables::MAC.may_spawn` | **widened**: no spawning on Android |
| `account_ipc.rs` device class, model, default name | **widened** with an Android twin (`android.rs`) |
| `ipc.rs` `KEYCHAIN_SERVICE` | gated to desktop and iOS (Android has no keychain service) |
| `keeper` `Cargo.toml` `keyring` | moved to the desktop-only table |
| `ipc.rs` `share_out: cfg!(target_os = "ios")` | kept: Android earns share-out with its own intent |
| `push_open_take`, `sign_out`'s pusher, `egress_list`'s push row, `unifiedpush_status`, `unifiedpush_confirm` | **widened** (98.4): `push_android.rs` — the tapped room, the UnifiedPush pusher removed at sign-out, the UnifiedPush server's row, where push stands and the distributor the person confirms (every other build answers `null`) |
| `push_ios`, `push_gateway_set`'s registration, `push_egress` | kept: the typed gateway is the iPhone's (APNs); Android's gateway is discovered on the distributor's server |
| `voice_ios`, `voice_island`, `voice_notify`, `voice_synthesizer`, `voice_ipc`, `voice_turn_models` | kept: Android voice is story 98.4; `voice_ipc`'s `AbsentPort` answers there |
| `web_auth.rs` `start`, `start_in`, `MatrixSignIn::open_url` | **widened** (rung 8): an Auth Tab on Android (`AuthTabPlugin.kt`) |
| `web_auth_apple.rs` | kept: `ASWebAuthenticationSession` is iOS's |
| `share_ios.rs` | kept: share-out is off on Android (`share_out` false) |
| `ipc.rs` `device_ulid` recording refusal | **widened**: quotes `MobileRecorder::refused` on iOS and Android alike |
| `sessions/lock.rs`, `browse.rs` | already Android-aware |

## Dependencies and licences

`cargo deny` never sees Gradle. Two checks stand in for it.

**What the build files declare.** `android_gradle_dependencies_are_licensed`
(`src-tauri/crates/keeper-core/tests/android_gradle_licences.rs`, Linux) reads
every `group:artifact:version` declared in `gen/android`'s Gradle build files
and in the Android libraries of the Tauri crates in `Cargo.lock` (`tauri`'s
`mobile/android`, each `tauri-plugin-*`'s `android/`) — test configurations
(`testImplementation`, `androidTestImplementation`) excepted, since they neither
ship nor run in keeper's build — and fails unless each
is a row below whose licence is one `src-tauri/deny.toml` allows; a GPL or
AGPL licence, an unknown one, an unlisted coordinate, or a row that no build
file declares any more fails too. Bumping a version means updating its row.

| Coordinate | Licence | Declared by |
|---|---|---|
| `androidx.activity:activity-ktx:1.10.1` | Apache-2.0 | `gen/android` app |
| `androidx.appcompat:appcompat:1.7.1` | Apache-2.0 | `gen/android` app |
| `androidx.browser:browser:1.10.0` | Apache-2.0 | `gen/android` app (Auth Tab) |
| `androidx.lifecycle:lifecycle-process:2.10.0` | Apache-2.0 | `gen/android` app |
| `androidx.webkit:webkit:1.14.0` | Apache-2.0 | `gen/android` app |
| `org.unifiedpush.android:connector:2.5.0` | Apache-2.0 | `gen/android` app (push) |
| `com.google.android.material:material:1.12.0` | Apache-2.0 | `gen/android` app |
| `com.android.tools.build:gradle:8.11.0` | Apache-2.0 | `gen/android` build script, `buildSrc` (build tooling) |
| `org.jetbrains.kotlin:kotlin-gradle-plugin:2.1.20` | Apache-2.0 | `gen/android` build script (build tooling; 2.1 because androidx.browser 1.10.0 brings `kotlin-stdlib` 2.1.20) |
| `androidx.appcompat:appcompat:1.6.0` | Apache-2.0 | tauri, tauri-plugin-notification, -dialog, -fs, -deep-link |
| `androidx.browser:browser:1.8.0` | Apache-2.0 | tauri-plugin-opener |
| `androidx.core:core-ktx:1.7.0` | Apache-2.0 | tauri |
| `androidx.core:core-ktx:1.9.0` | Apache-2.0 | tauri-plugin-notification, -opener, -dialog, -fs, -deep-link |
| `com.fasterxml.jackson.core:jackson-databind:2.15.3` | Apache-2.0 | tauri, tauri-plugin-notification, -opener |
| `com.google.android.material:material:1.7.0` | Apache-2.0 | tauri, tauri-plugin-notification, -dialog, -fs, -deep-link |

Outside the scan, also Apache-2.0: the Gradle wrapper (`gradle-wrapper.jar`,
committed by `tauri android init`) and the Gradle 8.14.3 distribution it
downloads.

## What Gradle resolves

**What ships.** The coordinates above pull in more, which only Gradle
resolves. After building the APK, CI's `android` job has Gradle list the
APK's runtime classpath (`./gradlew -q :app:dependencies --configuration
universalDebugRuntimeClasspath`, and the same for `universalRelease`, which
`bun run install:android` signs) and runs
`the_resolved_android_classpath_is_licensed` over that report: every artifact
in it, transitive ones included, must belong to a group below; a group nobody
listed, a line Gradle could not resolve, or a row nothing resolves any more
fails the job. A row is one Maven group, or a group and every group beneath it
when it ends in `.*`. Every row's licence is checked against
`src-tauri/deny.toml` on Linux too
(`every_group_gradle_may_resolve_carries_an_allowed_licence`). The list below
is what Gradle resolved on 2026-10-12 for both configurations (identical) —
again after story 98.4 added the UnifiedPush connector, in a scratch copy of
`gen/android`, where `the_resolved_android_classpath_is_licensed` passed over
the report; the job's first run is what confirms it on CI's runner.

| Group | Licence | What, pulled in by |
|---|---|---|
| `androidx.*` | Apache-2.0 | AndroidX — the app, tauri and every plugin |
| `com.fasterxml.jackson` | Apache-2.0 | `jackson-bom`, via `jackson-databind` |
| `com.fasterxml.jackson.core` | Apache-2.0 | `jackson-databind`, `-core`, `-annotations` — tauri, the notification and opener plugins |
| `com.google.android.material` | Apache-2.0 | `material` — the app, tauri, the plugins |
| `com.google.errorprone` | Apache-2.0 | `error_prone_annotations`, via AndroidX |
| `com.google.guava` | Apache-2.0 | `listenablefuture`, via AndroidX |
| `org.jetbrains` | Apache-2.0 | `annotations`, via the Kotlin standard library |
| `org.jetbrains.kotlin` | Apache-2.0 | `kotlin-stdlib`, `kotlin-bom` |
| `org.jetbrains.kotlinx` | Apache-2.0 | `kotlinx-coroutines-*`, `kotlinx-serialization-*`, via AndroidX |
| `org.jspecify` | Apache-2.0 | `jspecify`, via AndroidX |
| `org.unifiedpush.android` | Apache-2.0 | `connector` — the app (push) |
