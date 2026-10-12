//! What a phone or tablet build says it does differently: the "On this
//! iPhone" / "On this tablet" list in Settings → About (Story 13.7; Epic 66,
//! AD-204; Epic 98, story 98.3 #4, codemap Q12 (b)).
//!
//! The lines are composed here, per device, and reach the webview as
//! [`PlatformDisclosureVm`] over `platform_facts`; the front renders them and
//! decides nothing. Each list is mirrored one-to-one into its doc's
//! *Limitations* list — [`IPHONE_LINES`] into `docs/ios.md`, [`android`]'s
//! tablet wording into `docs/android.md` — and
//! `keeper-core/tests/platform_disclosure_mirrors_docs.rs` reads both files from
//! disk and fails when a list and its doc differ. Edit both together or neither.
//!
//! Project voice: sentence case, no exclamation marks, honest
//! consequence-naming; each line names a fact a person can act on — what the
//! device does differently, and what stays on the Mac (AD-201, AD-203).

use crate::vm::PlatformDisclosureVm;
use crate::voice::platform::VoicePlatform;

/// Which reduced build is speaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    /// keeper on iOS.
    Iphone,
    /// keeper on an Android device whose smallest width is at least 600 dp.
    AndroidTablet,
    /// keeper on any other Android device.
    AndroidPhone,
}

/// The iPhone's eight lines (rewritten by Epic 66, Story 66.6, after the phone
/// gained the folder; the first and seventh by Epic 67, Story 67.4, once a
/// spoken turn finished with the screen locked; the first by 98.1 for push; the
/// fifth by 98.2 for the sessions board).
///
/// The third line's fact is `phone_divergence_sentence` in
/// `keeper-sync/src/engine.rs` (AD-199), worded for a list rather than a status
/// row. The first line's two exceptions are `NO_BACKGROUND_SYNC_SENTENCE`'s
/// (push, AD-412; the voice banner, AD-207), in the list's voice. The eighth
/// line IS [`VoicePlatform::IOS`]'s `limits` (AD-193): the sentence read here
/// before switching listening on is the sentence shown beside the switch.
pub const IPHONE_LINES: [&str; 8] = [
    "keeper syncs and notifies about messages only while it's open. Two things still reach you while it is closed: with a build signed by your own paid Apple team and a push gateway you run, your agents' approval requests and your own agent's answers arrive as notifications through it, read on this phone (nothing else shows; the ids of other messages still pass through your gateway and Apple, and this phone discards them); and a voice session you left listening answers with the screen locked and posts what it heard as a banner from this phone.",
    "A folder you add here mirrors a remote your Mac already syncs and lives inside keeper's own container: every large file stays a pointer until you open it, and it syncs only when keeper opens, comes back in front or you pull to refresh — nothing watches the folder on a battery.",
    "Nothing is merged on a phone: a history this copy cannot fast-forward is named — how many commits this phone has that the remote does not — and you push them or reset this copy; the Mac merges.",
    "Notes are a synced folder, so they are here too: list, search, read and edit them, and a save is a commit that keeper pushes from this phone with its own engine — there is no git on a phone.",
    "The sessions board is here too: read a session's cards, move one with Move to…, and open an agent's session room to decide its approval; creating and archiving sessions stays on your Mac, with tasks, the self-hosted bridge runner, screen recording and the global summon hotkey. Bots talks to a model here, but the drive tools live on your Mac.",
    "Updates arrive by reinstalling keeper; its signature renews every 7 days.",
    "Listening for the wake phrase starts in keeper, and then keeps working with another app in front and with the screen locked; the answer is spoken there too, and saying \"stop\" (or the stop word you chose) ends it. Speech is recognised on this phone and never sent to a server.",
    VoicePlatform::IOS.limits,
];

/// The Android lines for a device called `noun` ("tablet" or "phone"):
/// no agent runs on it, no screen recording or transcription, the board reads,
/// moves and decides as on the phone, push arrives through the person's
/// UnifiedPush distributor ([`android_push_line`]), and voice works while
/// keeper is in front (`voice::android::disclosure_line`).
pub fn android(noun: &str) -> Vec<String> {
    vec![
        format!("No agent runs on this {noun}: your agents run on your Mac or your server, and keeper here talks to them."),
        format!("A folder you add here mirrors a remote your Mac already syncs and lives inside keeper's own storage: it syncs only when keeper opens, comes back in front or you pull to refresh, and nothing is merged on this {noun} — the Mac merges."),
        "The sessions board is here: read a session's cards, move one with Move to…, and open an agent's session room to decide its approval; creating and archiving sessions stays on your Mac. Bots talks to a model here, but the drive tools live on your Mac.".to_owned(),
        format!("No screen recording and no transcription on this {noun}."),
        android_push_line(noun),
        crate::voice::android::disclosure_line(noun),
        "Updates arrive by installing a new build, signed with your own key and sideloaded.".to_owned(),
    ]
}

/// What reaches an Android device called `noun` while keeper is not in front
/// (98.4, AD-414): push through the person's UnifiedPush distributor, read by
/// keeper itself while Android keeps it running, and only a sign that
/// something arrived once Android has stopped it
/// (`agents::unifiedpush`). Also the note under Settings → Notifications'
/// push status there.
pub fn android_push_line(noun: &str) -> String {
    format!(
        "keeper syncs and notifies about messages only while it is open. With a UnifiedPush distributor whose server is also a Matrix push gateway, your agents' approval requests and your own agent's answers reach this {noun} while keeper runs in the background, read on this {noun} (nothing else shows; the ids of other messages still pass through that server, and this {noun} discards them); once Android has stopped keeper, a push only says that something arrived."
    )
}

/// The disclosure `device` shows: its heading, its lines, and the doc that
/// says the same thing at length.
pub fn disclosure(device: Device) -> PlatformDisclosureVm {
    match device {
        Device::Iphone => PlatformDisclosureVm {
            heading: "On this iPhone".to_owned(),
            lines: IPHONE_LINES.iter().map(|line| (*line).to_owned()).collect(),
            docs_url: "https://github.com/tgorka/keeper/blob/main/docs/ios.md".to_owned(),
            docs_label: "What's different on iPhone".to_owned(),
        },
        Device::AndroidTablet | Device::AndroidPhone => {
            let noun = if device == Device::AndroidTablet {
                "tablet"
            } else {
                "phone"
            };
            PlatformDisclosureVm {
                heading: format!("On this {noun}"),
                lines: android(noun),
                docs_url: "https://github.com/tgorka/keeper/blob/main/docs/android.md".to_owned(),
                docs_label: "What's different on Android".to_owned(),
            }
        }
    }
}
