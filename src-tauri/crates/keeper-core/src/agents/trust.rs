//! Who may decide an approval, and from which device (AD-395, R30, R87,
//! R88; FR-798).
//!
//! A session room lets a person send `m.room.encrypted` at power 0, so the
//! homeserver cannot tell a decision from free text: whether one counts is
//! the owning host's call, after decryption, over the facts below. A
//! decision counts only from a person — never an agent — who reads the
//! session and is one of the action's approvers, sealed by that person's own
//! device, a device the person's cross-signing identity signed, whose master
//! key is the one this host trusts for them. At T4 two more facts hold: the
//! decider is the requester at the head of the record's `dispatch_chain`
//! (S-28), and the device is not one this host's own process runs (S-22).
//!
//! The trusted master key comes from the host, never from the network: a
//! headless host's `[[trust]].master_key`, written by a person after they
//! compared fingerprints (keeper never pins by itself, never trusts on first
//! use); the desktop's signed-in account while its own identity is verified
//! on this device (R87). A person with no such key is "not pinned", however
//! well their device is verified.

use matrix_sdk::encryption::vodozemac::{Ed25519PublicKey, Ed25519Signature};
use matrix_sdk::ruma::{CanonicalJsonValue, UserId};

use crate::agents::agentd::TrustEntry;

/// Every fact one decision is judged on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustFacts {
    /// Who sent the decision.
    pub sender: String,
    /// The device that sent it, as the event's encryption names it.
    pub device: Option<String>,
    /// Whether the sender's own device sealed it: encrypted, and not a
    /// Megolm session of another user's device.
    pub event_encrypted: bool,
    /// Whether the sender is an agent's user.
    pub sender_is_agent: bool,
    /// Whether the sender reads the session as its label is now.
    pub sender_in_label: bool,
    /// Whether the sender reads the label the record was parked under.
    pub sender_in_approvers: bool,
    /// The head of the record's `dispatch_chain`: the person who asked.
    pub requester: Option<String>,
    /// Whether the device is one this host's own process runs (R87).
    pub device_is_this_process: bool,
    /// Whether the sender's cross-signing identity signed the device.
    pub device_cross_signed_by_owner: bool,
    /// The sender's master key as the homeserver publishes it now.
    pub owner_master_key: Option<String>,
    /// The master key this host trusts for the sender.
    pub pinned_master_key: Option<String>,
}

/// Whether a decision counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trust {
    Verified,
    /// It does not, and why — the sentence an ignored decision is logged
    /// with.
    Unverified {
        reason: String,
    },
}

/// An agent's decision.
pub const AGENT: &str = "an agent does not decide an approval; a person does";
/// A decision in clear, or one whose Megolm session is another user's.
pub const UNSEALED: &str = "the decision was not sealed by its sender's own device";
/// A sender who does not read the session now.
pub const NOT_A_READER: &str = "the sender does not read this session";
/// A sender who did not read the session when the action was parked.
pub const NOT_AN_APPROVER: &str = "the sender is not one of this action's approvers";
/// A decision whose encryption names no device.
pub const NO_DEVICE: &str = "the decision does not say which device sent it";
/// A device its owner's cross-signing identity did not sign.
pub const UNSIGNED_DEVICE: &str =
    "the device it came from is not verified by its owner's cross-signing identity";
/// No master key this host trusts for the sender.
pub const NOT_PINNED: &str = "not pinned: this host trusts no master key of this person";
/// A person who publishes no cross-signing identity.
pub const NO_IDENTITY: &str = "the person publishes no cross-signing identity";
/// The published master key is not the trusted one: reset, or not theirs.
pub const KEY_MOVED: &str = "the person's master key is not the one this host trusts";
/// A T4 decision from a device of this host's own process.
pub const THIS_DEVICE: &str =
    "decide on another device: this one runs the agent that asks, so it cannot be the one that agrees";
/// A T4 record that names nobody who asked.
pub const NO_REQUESTER: &str = "nobody can decide this: it names nobody who asked";
/// The sender's keys could not be read whole from their homeserver for
/// this decision: no earlier answer stands in for it (R182).
pub const KEYS_UNKNOWN: &str =
    "unknown: the person's keys could not be read from their homeserver just now";

/// Whether the decision described by `facts` counts on a record of `tier`.
/// The first fact that fails is the reason.
pub fn decide_trust(facts: &TrustFacts, tier: u8) -> Trust {
    let unverified = |reason: &str| Trust::Unverified {
        reason: reason.to_owned(),
    };
    let irreversible = tier >= 4;
    if facts.sender_is_agent {
        return unverified(AGENT);
    }
    if !facts.event_encrypted {
        return unverified(UNSEALED);
    }
    if !facts.sender_in_label {
        return unverified(NOT_A_READER);
    }
    if !facts.sender_in_approvers {
        return unverified(NOT_AN_APPROVER);
    }
    if irreversible {
        match &facts.requester {
            None => return unverified(NO_REQUESTER),
            Some(requester) if *requester != facts.sender => {
                return unverified(&only(requester));
            }
            Some(_) => {}
        }
    }
    if facts.device.is_none() {
        return unverified(NO_DEVICE);
    }
    if !facts.device_cross_signed_by_owner {
        return unverified(UNSIGNED_DEVICE);
    }
    let Some(pinned) = &facts.pinned_master_key else {
        return unverified(NOT_PINNED);
    };
    let Some(published) = &facts.owner_master_key else {
        return unverified(NO_IDENTITY);
    };
    if published != pinned {
        return unverified(KEY_MOVED);
    }
    if irreversible && facts.device_is_this_process {
        return unverified(THIS_DEVICE);
    }
    Trust::Verified
}

/// Why a T4 decision from anyone but `requester` does not count.
pub fn only(requester: &str) -> String {
    format!("only {requester} can decide this")
}

/// What the homeserver publishes of a sender's device and identity, read
/// from one fresh `/keys/query` answer ([`published_in`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Published {
    /// Whether the device is signed by the self-signing key that
    /// [`Published::master_key`] signed, in that same answer.
    pub cross_signed_by_owner: bool,
    /// The owner's master key, `ed25519:<unpadded base64>`.
    pub master_key: Option<String>,
}

/// The parts of one `/keys/query` answer about one person, as it came.
#[derive(Debug, Clone, Default)]
pub struct KeysAnswer {
    /// Whether the answer names any homeserver it could not ask.
    pub failed: bool,
    pub master_key: Option<serde_json::Value>,
    pub self_signing_key: Option<serde_json::Value>,
    /// The asked device's keys, when the answer has them.
    pub device: Option<serde_json::Value>,
}

/// What `answer` publishes of `user` and their `device`, judged on that
/// answer alone (R182): the master key it carries, and whether the device
/// is signed by a self-signing key that this very master key signed —
/// never a key or a verdict from another read, nor from the crypto store.
/// [`KEYS_UNKNOWN`] when the answer is not whole: a homeserver it could not
/// ask, or a master key that is not one ed25519 key of `user`'s.
pub fn published_in(
    answer: &KeysAnswer,
    user: &UserId,
    device: Option<&str>,
) -> Result<Published, &'static str> {
    if answer.failed {
        return Err(KEYS_UNKNOWN);
    }
    let Some(master) = &answer.master_key else {
        return Ok(Published::default());
    };
    let (master_id, master_key) = signing_key(master, user, "master").ok_or(KEYS_UNKNOWN)?;
    let cross_signed = || -> Option<()> {
        let self_signing = answer.self_signing_key.as_ref()?;
        let (id, key) = signing_key(self_signing, user, "self_signing")?;
        signed_by(self_signing, user, &master_id, &master_key)?;
        let keys = answer.device.as_ref()?;
        let named = keys["user_id"].as_str() == Some(user.as_str())
            && keys["device_id"]
                .as_str()
                .is_some_and(|id| Some(id) == device);
        named.then_some(())?;
        signed_by(keys, user, &id, &key)
    };
    Ok(Published {
        cross_signed_by_owner: device.is_some() && cross_signed().is_some(),
        master_key: Some(format!("ed25519:{}", master_key.to_base64())),
    })
}

/// The one ed25519 key of `user`'s cross-signing key `key` for `usage`, and
/// its key id.
fn signing_key(
    key: &serde_json::Value,
    user: &UserId,
    usage: &str,
) -> Option<(String, Ed25519PublicKey)> {
    if key["user_id"].as_str() != Some(user.as_str())
        || key["usage"].as_array()?.iter().all(|used| used != usage)
    {
        return None;
    }
    let keys = key["keys"].as_object()?;
    let [(id, value)] = keys.iter().collect::<Vec<_>>()[..] else {
        return None;
    };
    let base64 = id.strip_prefix("ed25519:")?;
    (value.as_str() == Some(base64)).then_some(())?;
    Some((id.clone(), Ed25519PublicKey::from_base64(base64).ok()?))
}

/// Whether `object` carries `user`'s valid signature by `key` (`key_id`)
/// over its canonical JSON without `signatures` and `unsigned`.
fn signed_by(
    object: &serde_json::Value,
    user: &UserId,
    key_id: &str,
    key: &Ed25519PublicKey,
) -> Option<()> {
    let signature = object["signatures"][user.as_str()][key_id].as_str()?;
    let signature = Ed25519Signature::from_base64(signature).ok()?;
    let mut signed = object.clone();
    let fields = signed.as_object_mut()?;
    fields.remove("signatures");
    fields.remove("unsigned");
    let canonical = CanonicalJsonValue::try_from(signed).ok()?;
    key.verify(canonical.to_string().as_bytes(), &signature)
        .ok()
}

/// One signed-in account of the desktop (R87): the person whose own
/// identity, while verified on this device, is the Mac's trust anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnAccount {
    pub user: matrix_sdk::ruma::OwnedUserId,
    /// This app's messenger device of that account.
    pub device_id: String,
    /// Whether the account's own cross-signing identity is verified on this
    /// device.
    pub own_identity_verified: bool,
    /// Its master key, `ed25519:<unpadded base64>`.
    pub master_key: Option<String>,
}

/// Where a host's trusted master keys come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    /// A headless host: `agentd.toml`'s `[[trust]]`, pinned by a person.
    Pinned(Vec<TrustEntry>),
    /// The desktop: exactly its signed-in accounts, each while verified
    /// (R87).
    Desktop(Vec<OwnAccount>),
}

impl Anchor {
    /// The master key this host trusts for `user`, if any.
    pub fn pinned(&self, user: &UserId) -> Option<&str> {
        match self {
            Anchor::Pinned(entries) => entries
                .iter()
                .find(|entry| entry.user == user)
                .and_then(|entry| entry.master_key.as_deref()),
            Anchor::Desktop(accounts) => accounts
                .iter()
                .find(|account| account.user == user && account.own_identity_verified)
                .and_then(|account| account.master_key.as_deref()),
        }
    }

    /// Whether `user`'s `device` is one this host's own process runs: on
    /// the desktop, the person's messenger device in this app; on a
    /// headless host, never a person's.
    pub fn this_process(&self, user: &UserId, device: &str) -> bool {
        match self {
            Anchor::Pinned(_) => false,
            Anchor::Desktop(accounts) => accounts
                .iter()
                .any(|account| account.user == user && account.device_id == device),
        }
    }
}

/// A master key as a person compares it by eye: its base64 in groups of
/// four, as clients print it. Takes the pinned form `ed25519:<base64>` or
/// the bare base64.
pub fn fingerprint(master_key: &str) -> String {
    let key = master_key.strip_prefix("ed25519:").unwrap_or(master_key);
    key.as_bytes()
        .chunks(4)
        .map(|group| String::from_utf8_lossy(group).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// How a `[[trust]]` person's pin stands against what their homeserver
/// publishes now (R88).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinState {
    /// The published key is the pinned one.
    Matches,
    /// The published key is another: their identity was reset, or this is
    /// not their key. No decision of theirs counts until a person re-pins.
    Differs,
    /// Nothing is pinned yet: no decision of theirs counts.
    NotPinned,
    /// A key is pinned and none is published.
    NotPublished,
    /// The homeserver could not be asked.
    Unknown,
}

impl PinState {
    /// `published` (`None`: no identity) against `pinned`.
    pub fn of(published: Option<&str>, pinned: Option<&str>) -> PinState {
        match (published, pinned) {
            (_, None) => PinState::NotPinned,
            (None, Some(_)) => PinState::NotPublished,
            (Some(published), Some(pinned)) if published == pinned => PinState::Matches,
            (Some(_), Some(_)) => PinState::Differs,
        }
    }

    pub fn as_word(self) -> &'static str {
        match self {
            PinState::Matches => "matches",
            PinState::Differs => "differs",
            PinState::NotPinned => "not pinned",
            PinState::NotPublished => "not published",
            PinState::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::OwnedUserId;

    use super::*;

    const TGORKA: &str = "@tgorka:example.org";
    const MARTA: &str = "@marta:example.org";
    const KEY: &str = "ed25519:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdE";
    const RESET: &str = "ed25519:ZZZZEfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdE";

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    /// tgorka on his cross-signed phone, pinned, asking himself: every fact
    /// holds.
    fn verified() -> TrustFacts {
        TrustFacts {
            sender: TGORKA.to_owned(),
            device: Some("PHONE".to_owned()),
            event_encrypted: true,
            sender_is_agent: false,
            sender_in_label: true,
            sender_in_approvers: true,
            requester: Some(TGORKA.to_owned()),
            device_is_this_process: false,
            device_cross_signed_by_owner: true,
            owner_master_key: Some(KEY.to_owned()),
            pinned_master_key: Some(KEY.to_owned()),
        }
    }

    fn reason(facts: &TrustFacts, tier: u8) -> String {
        match decide_trust(facts, tier) {
            Trust::Verified => panic!("verified: {facts:?} at T{tier}"),
            Trust::Unverified { reason } => reason,
        }
    }

    /// 93.3 AC1: every fact holds → verified at every tier; each single fact
    /// false → unverified, naming it.
    #[test]
    fn only_a_verified_device_of_a_reader_decides() {
        for tier in 0..=4 {
            assert_eq!(decide_trust(&verified(), tier), Trust::Verified, "T{tier}");
        }
        type Flip = fn(&mut TrustFacts);
        let flips: [(Flip, &str); 9] = [
            (|f| f.sender_is_agent = true, AGENT),
            (|f| f.event_encrypted = false, UNSEALED),
            (|f| f.sender_in_label = false, NOT_A_READER),
            (|f| f.sender_in_approvers = false, NOT_AN_APPROVER),
            (|f| f.device = None, NO_DEVICE),
            // The unverified device: a fresh login nobody signed.
            (|f| f.device_cross_signed_by_owner = false, UNSIGNED_DEVICE),
            // A person with no pin, however well their device is verified.
            (|f| f.pinned_master_key = None, NOT_PINNED),
            (|f| f.owner_master_key = None, NO_IDENTITY),
            // The matching person whose master key was reset since the pin.
            (|f| f.owner_master_key = Some(RESET.to_owned()), KEY_MOVED),
        ];
        for tier in [2, 3, 4] {
            for (flip, named) in flips {
                let mut facts = verified();
                flip(&mut facts);
                assert_eq!(reason(&facts, tier), named, "T{tier}");
            }
        }
    }

    /// 93.3 AC1 at T4 (S-22, S-28): a reader who is not the requester, and
    /// the requester on a device of this host's own process, are each named;
    /// below T4 neither fact matters.
    #[test]
    fn an_irreversible_action_is_the_requesters_on_another_device() {
        let mut marta = verified();
        marta.sender = MARTA.to_owned();
        assert_eq!(reason(&marta, 4), only(TGORKA));
        assert_eq!(only(TGORKA), "only @tgorka:example.org can decide this");
        assert_eq!(decide_trust(&marta, 3), Trust::Verified);

        let mut in_process = verified();
        in_process.device_is_this_process = true;
        assert_eq!(reason(&in_process, 4), THIS_DEVICE);
        assert_eq!(decide_trust(&in_process, 3), Trust::Verified);

        let mut nobody = verified();
        nobody.requester = None;
        assert_eq!(reason(&nobody, 4), NO_REQUESTER);
        assert_eq!(decide_trust(&nobody, 3), Trust::Verified);
    }

    /// A different person's device: tgorka's key pinned, Marta's published.
    #[test]
    fn a_different_persons_device_is_not_trusted_by_another_persons_pin() {
        let anchor = Anchor::Pinned(vec![TrustEntry {
            user: user(TGORKA),
            master_key: Some(KEY.to_owned()),
            proxy: None,
        }]);
        let mut marta = verified();
        marta.sender = MARTA.to_owned();
        marta.owner_master_key = Some(RESET.to_owned());
        marta.pinned_master_key = anchor.pinned(&user(MARTA)).map(str::to_owned);
        assert_eq!(reason(&marta, 2), NOT_PINNED);
        assert_eq!(anchor.pinned(&user(TGORKA)), Some(KEY));
        assert!(!anchor.this_process(&user(TGORKA), "PHONE"));
        let unpinned = Anchor::Pinned(vec![TrustEntry {
            user: user(TGORKA),
            master_key: None,
            proxy: None,
        }]);
        assert_eq!(unpinned.pinned(&user(TGORKA)), None);
    }

    /// R87: on the desktop the signed-in account is trusted exactly while
    /// its own identity is verified here, and its messenger device is this
    /// process's: an own-device T4 decision is refused, its phone's counts.
    #[test]
    fn the_desktop_trusts_its_own_verified_account() {
        let account = |verified: bool| OwnAccount {
            user: user(TGORKA),
            device_id: "MAC".to_owned(),
            own_identity_verified: verified,
            master_key: Some(KEY.to_owned()),
        };
        let anchor = Anchor::Desktop(vec![account(true)]);
        assert_eq!(anchor.pinned(&user(TGORKA)), Some(KEY));
        assert_eq!(anchor.pinned(&user(MARTA)), None);
        assert!(anchor.this_process(&user(TGORKA), "MAC"));
        assert!(!anchor.this_process(&user(TGORKA), "PHONE"));
        assert!(!anchor.this_process(&user(MARTA), "MAC"));

        let facts = |device: &str| {
            let mut facts = verified();
            facts.device = Some(device.to_owned());
            facts.pinned_master_key = anchor.pinned(&user(TGORKA)).map(str::to_owned);
            facts.device_is_this_process = anchor.this_process(&user(TGORKA), device);
            facts
        };
        assert_eq!(reason(&facts("MAC"), 4), THIS_DEVICE);
        assert_eq!(decide_trust(&facts("MAC"), 3), Trust::Verified);
        assert_eq!(decide_trust(&facts("PHONE"), 4), Trust::Verified);

        let unverified = Anchor::Desktop(vec![account(false)]);
        assert_eq!(unverified.pinned(&user(TGORKA)), None);
    }

    /// 93.3 AC7 (pure): a fingerprint is the key's own base64 in fours, and
    /// a pin's state is read against what is published.
    #[test]
    fn a_fingerprint_is_the_keys_own_and_status_says_if_it_moved() {
        assert_eq!(
            fingerprint(KEY),
            "AbCd EfGh IjKl MnOp QrSt UvWx Yz01 2345 6789 +/Ab CdE"
        );
        assert_eq!(fingerprint(&KEY["ed25519:".len()..]), fingerprint(KEY));
        assert_eq!(PinState::of(Some(KEY), Some(KEY)), PinState::Matches);
        assert_eq!(PinState::of(Some(RESET), Some(KEY)), PinState::Differs);
        assert_eq!(PinState::of(Some(KEY), None), PinState::NotPinned);
        assert_eq!(PinState::of(None, None), PinState::NotPinned);
        assert_eq!(PinState::of(None, Some(KEY)), PinState::NotPublished);
        assert_eq!(PinState::NotPinned.as_word(), "not pinned");
        assert_eq!(PinState::Differs.as_word(), "differs");
    }

    use matrix_sdk::encryption::vodozemac::Ed25519SecretKey;
    use serde_json::{json, Value};

    /// `object` with `user`'s signature by `key` over its canonical JSON.
    fn sign(mut object: Value, user: &str, key: &Ed25519SecretKey) -> Value {
        let canonical = CanonicalJsonValue::try_from(object.clone())
            .expect("canonical")
            .to_string();
        let id = format!("ed25519:{}", key.public_key().to_base64());
        let signature = key.sign(canonical.as_bytes()).to_base64();
        object["signatures"] = json!({ user: { id: signature } });
        object
    }

    fn cross_signing(usage: &str, key: &Ed25519SecretKey) -> Value {
        let public = key.public_key().to_base64();
        json!({
            "user_id": TGORKA,
            "usage": [usage],
            "keys": { format!("ed25519:{public}"): public },
        })
    }

    /// One cross-signing identity of tgorka's: its master and self-signing
    /// keys.
    struct Identity {
        master: Ed25519SecretKey,
        self_signing: Ed25519SecretKey,
    }

    impl Identity {
        fn new() -> Identity {
            Identity {
                master: Ed25519SecretKey::new(),
                self_signing: Ed25519SecretKey::new(),
            }
        }

        fn pin(&self) -> String {
            format!("ed25519:{}", self.master.public_key().to_base64())
        }

        fn master_key(&self) -> Value {
            cross_signing("master", &self.master)
        }

        fn self_signing_key(&self) -> Value {
            sign(
                cross_signing("self_signing", &self.self_signing),
                TGORKA,
                &self.master,
            )
        }

        /// The device `id`'s keys, signed by this identity.
        fn device(&self, id: &str) -> Value {
            sign(
                json!({
                    "user_id": TGORKA,
                    "device_id": id,
                    "algorithms": ["m.megolm.v1.aes-sha2"],
                    "keys": { format!("ed25519:{id}"): "AAAA" },
                }),
                TGORKA,
                &self.self_signing,
            )
        }

        /// A whole answer about this identity and `device`'s keys.
        fn answer(&self, device: Value) -> KeysAnswer {
            KeysAnswer {
                failed: false,
                master_key: Some(self.master_key()),
                self_signing_key: Some(self.self_signing_key()),
                device: Some(device),
            }
        }
    }

    fn published(answer: &KeysAnswer) -> Result<Published, &'static str> {
        published_in(answer, &user(TGORKA), Some("PHONE"))
    }

    /// R182: one answer is judged on its own. A device signed by the
    /// self-signing key its master key signed is cross-signed; an answer
    /// that names a homeserver it could not ask is unknown, however whole
    /// the rest; an identity reset since the pin always refuses.
    #[test]
    fn a_device_is_cross_signed_only_under_the_master_key_of_the_same_answer() {
        let a = Identity::new();
        let whole = a.answer(a.device("PHONE"));
        assert_eq!(
            published(&whole),
            Ok(Published {
                cross_signed_by_owner: true,
                master_key: Some(a.pin()),
            })
        );
        assert_eq!(
            published_in(&whole, &user(TGORKA), None),
            Ok(Published {
                cross_signed_by_owner: false,
                master_key: Some(a.pin()),
            })
        );
        let failed = KeysAnswer {
            failed: true,
            ..whole.clone()
        };
        assert_eq!(published(&failed), Err(KEYS_UNKNOWN));

        // R3-01: identity B replaced A while keeper asked; B's device, under
        // B's self-signing key, is never cross-signed beside A's master key.
        let b = Identity::new();
        let mixed = KeysAnswer {
            self_signing_key: Some(b.self_signing_key()),
            ..a.answer(b.device("PHONE"))
        };
        assert!(!published(&mixed).expect("whole").cross_signed_by_owner);
        let mixed = a.answer(b.device("PHONE"));
        assert!(!published(&mixed).expect("whole").cross_signed_by_owner);
        // A self-signing key A's master key did not sign.
        let unsigned = KeysAnswer {
            self_signing_key: Some(cross_signing("self_signing", &a.self_signing)),
            ..a.answer(a.device("PHONE"))
        };
        assert!(!published(&unsigned).expect("whole").cross_signed_by_owner);
        // Another device's keys are not this one's.
        assert!(
            !published(&a.answer(a.device("LAPTOP")))
                .expect("whole")
                .cross_signed_by_owner
        );

        // A genuine reset: B's device, cross-signed by B, under a pin of A.
        let reset = published(&b.answer(b.device("PHONE"))).expect("whole");
        assert!(reset.cross_signed_by_owner);
        let mut facts = verified();
        facts.device_cross_signed_by_owner = reset.cross_signed_by_owner;
        facts.owner_master_key = reset.master_key;
        facts.pinned_master_key = Some(a.pin());
        assert_eq!(reason(&facts, 2), KEY_MOVED);
    }

    /// R182: no master key in the answer is no identity; a master key that
    /// is not one ed25519 key of the person's is no answer.
    #[test]
    fn a_missing_identity_is_none_and_a_malformed_one_is_unknown() {
        let a = Identity::new();
        let none = KeysAnswer {
            master_key: None,
            ..a.answer(a.device("PHONE"))
        };
        assert_eq!(published(&none), Ok(Published::default()));
        let mut marta = a.master_key();
        marta["user_id"] = json!(MARTA);
        let mut two = a.master_key();
        two["keys"]["ed25519:BBBB"] = json!("BBBB");
        let mut usage = a.master_key();
        usage["usage"] = json!(["self_signing"]);
        for master in [marta, two, usage] {
            let answer = KeysAnswer {
                master_key: Some(master),
                ..a.answer(a.device("PHONE"))
            };
            assert_eq!(published(&answer), Err(KEYS_UNKNOWN));
        }
    }
}
