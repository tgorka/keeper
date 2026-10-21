//! Android push's pure half (Epic 98, story 98.4 #1, #3; the epic's Q11,
//! Q12): the UnifiedPush pusher, the endpoint's grammar, the discovery
//! answer, what a push names, the distributor keeper registers with, and what
//! a notification posts and offers on Android — `keeper_core::agents::unifiedpush`.
//! The request half of discovery is `unifiedpush_gateway_discovery.rs`.

use keeper_core::agents::push::{
    action_decision, tap_target, ActionRefusal, ActionTap, Category, PushApproval, PushOpenVm,
    PusherSpec, Shown, FINISH_IN_KEEPER,
};
use keeper_core::agents::unifiedpush::*;
use serde_json::json;

const ACCOUNT: &str = "01J0000000000000000000000A";
const ROOM: &str = "!session:example.org";
const NTFY: &str = "https://ntfy.example/upAbc?up=1";

fn endpoint(raw: &str) -> Endpoint {
    Endpoint::parse(raw).expect("an endpoint keeper registers")
}

/// 98.4 #1: the Android pusher is an `http` pusher whose pushkey is the
/// endpoint, whose `url` is the discovered gateway and whose format is
/// `event_id_only`, under the Android app id — and it carries nothing
/// else for the homeserver to forward: no APNs payload, no account.
#[test]
fn unifiedpush_pusher_spec() {
    let ntfy = endpoint(NTFY);
    let gateway = gateway_of_answer(&ntfy, 200, br#"{"unifiedpush":{"gateway":"matrix"}}"#)
        .expect("ntfy is a Matrix gateway");
    let spec = PusherSpec::unifiedpush(gateway, ntfy.clone(), ACCOUNT, "keeper on Tab S9");
    let pusher = serde_json::to_value(spec.pusher()).expect("serialises");
    assert_eq!(pusher["kind"], "http");
    assert_eq!(pusher["app_id"], APP_ID);
    assert_eq!(pusher["pushkey"], NTFY);
    assert_eq!(pusher["device_display_name"], "keeper on Tab S9");
    assert_eq!(
        pusher["data"],
        json!({
            "url": "https://ntfy.example/_matrix/push/v1/notify",
            "format": "event_id_only",
        })
    );
    assert_eq!(spec.ids(), ntfy.pusher_ids());
    assert_eq!(
        ntfy.pusher_ids(),
        matrix_sdk::ruma::api::client::push::PusherIds::new(NTFY.to_owned(), APP_ID.to_owned())
    );
}

/// The endpoint is `https://`, carries no user name or password and fits
/// a pushkey; every refusal names the host and never the endpoint, whose
/// path is a capability. The probe keeps the endpoint's host and port
/// and nothing of its path or query.
#[test]
fn an_endpoint_is_https_and_only_its_host_is_ever_said() {
    let ntfy = endpoint(" https://ntfy.example:8443/upAbc?up=1 ");
    assert_eq!(ntfy.host(), "ntfy.example:8443");
    assert_eq!(
        ntfy.probe().as_str(),
        "https://ntfy.example:8443/_matrix/push/v1/notify"
    );
    assert!(!format!("{ntfy:?}").contains("upAbc"));

    assert_eq!(
        Endpoint::parse("http://ntfy.example/upAbc"),
        Err(EndpointRefusal::NotHttps {
            host: "ntfy.example".to_owned(),
            scheme: "http".to_owned()
        })
    );
    assert_eq!(
        Endpoint::parse("https://me:pw@ntfy.example/upAbc"),
        Err(EndpointRefusal::Credentials {
            host: "ntfy.example".to_owned()
        })
    );
    let long = format!("https://ntfy.example/{}", "a".repeat(PUSHKEY_MAX));
    assert_eq!(
        Endpoint::parse(&long),
        Err(EndpointRefusal::TooLong {
            host: "ntfy.example".to_owned()
        })
    );
    let fits = format!("https://ntfy.example/{}", "a".repeat(PUSHKEY_MAX - 21));
    assert_eq!(
        Endpoint::parse(&fits).map(|e| e.as_str().len()),
        Ok(PUSHKEY_MAX)
    );
    assert_eq!(Endpoint::parse("upAbc"), Err(EndpointRefusal::NotAnAddress));
    for refusal in [
        Endpoint::parse("http://ntfy.example/upAbc"),
        Endpoint::parse("https://me:pw@ntfy.example/upAbc"),
        Endpoint::parse(&long),
    ] {
        let sentence = refusal.expect_err("refused").to_string();
        assert!(sentence.contains("ntfy.example"), "{sentence}");
        assert!(
            !sentence.contains("upAbc") && !sentence.contains("pw"),
            "{sentence}"
        );
    }
}

/// 98.4 #2, the answer half: only a 200 whose JSON names a Matrix
/// UnifiedPush gateway is one; the gateway is the endpoint's own server
/// at the push path. Every other answer refuses, naming the host.
#[test]
fn only_a_matrix_answer_makes_the_endpoints_server_the_gateway() {
    let ntfy = endpoint(NTFY);
    let matrix = br#"{"unifiedpush":{"gateway":"matrix"}}"#;
    assert_eq!(
        gateway_of_answer(&ntfy, 200, matrix).map(|g| g.as_str().to_owned()),
        Ok("https://ntfy.example/_matrix/push/v1/notify".to_owned())
    );
    let host = || "ntfy.example".to_owned();
    assert_eq!(
        gateway_of_answer(&ntfy, 404, matrix),
        Err(DiscoveryRefusal::Status {
            host: host(),
            status: 404
        })
    );
    assert_eq!(
        gateway_of_answer(&ntfy, 301, matrix),
        Err(DiscoveryRefusal::Status {
            host: host(),
            status: 301
        })
    );
    for not_matrix in [
        &b"<html>ntfy</html>"[..],
        br#"{"unifiedpush":{}}"#,
        br#"{"gateway":"matrix"}"#,
        br#"{"unifiedpush":{"gateway":true}}"#,
    ] {
        assert_eq!(
            gateway_of_answer(&ntfy, 200, not_matrix),
            Err(DiscoveryRefusal::NotMatrix { host: host() })
        );
    }
    assert_eq!(
        gateway_of_answer(&ntfy, 200, br#"{"unifiedpush":{"gateway":"webpush"}}"#),
        Err(DiscoveryRefusal::OtherGateway {
            host: host(),
            kind: "webpush".to_owned()
        })
    );
}

/// The gateway's forwarded notification names its room and event;
/// anything without both, or that is not that JSON, names nothing.
#[test]
fn a_push_names_the_room_and_event_of_the_homeservers_notification() {
    let forwarded = json!({"notification": {
        "event_id": "$ask:example.org", "room_id": ROOM, "counts": {"unread": 1},
        "devices": [{"app_id": APP_ID, "pushkey": NTFY}],
    }});
    assert_eq!(
        pushed_event(forwarded.to_string().as_bytes()),
        Some(PushedEvent {
            room_id: matrix_sdk::ruma::OwnedRoomId::try_from(ROOM).expect("room"),
            event_id: matrix_sdk::ruma::OwnedEventId::try_from("$ask:example.org").expect("event"),
        })
    );
    for nothing in [
        json!({"notification": {"counts": {"unread": 0}}}),
        json!({"notification": {"room_id": ROOM}}),
        json!({"notification": {"room_id": "not a room", "event_id": "$e:x"}}),
        json!({"room_id": ROOM, "event_id": "$e:x"}),
    ] {
        assert_eq!(
            pushed_event(nothing.to_string().as_bytes()),
            None,
            "{nothing}"
        );
    }
    assert_eq!(pushed_event(b"not json"), None);
}

fn distributor(package: &str, label: &str) -> Distributor {
    Distributor {
        package: package.to_owned(),
        label: label.to_owned(),
    }
}

/// R98UP1-02: keeper registers with a distributor only after the person
/// confirmed that one on this device — never the only one installed on its
/// own, never one of several — and a different distributor later asks
/// again. The confirmation is kept on this device and confirms only a
/// distributor keeper asked about.
#[test]
fn keeper_registers_only_with_a_distributor_the_person_confirmed() {
    let ntfy = distributor("io.heckel.ntfy", "ntfy");
    let nextpush = distributor("org.unifiedpush.distributor.nextpush", "NextPush");
    let both = [ntfy.clone(), nextpush.clone()];

    // No confirmation: asked, whether one or several are installed.
    assert_eq!(
        choose_distributor(None, std::slice::from_ref(&ntfy)),
        DistributorChoice::Ask(vec![ntfy.clone()])
    );
    assert_eq!(
        choose_distributor(None, &both),
        DistributorChoice::Ask(both.to_vec())
    );
    assert_eq!(choose_distributor(None, &[]), DistributorChoice::None);

    // The person confirms ntfy in Settings; it is kept on this device.
    let data = tempfile::tempdir().expect("a data directory");
    assert_eq!(confirmed_distributor(data.path()), None);
    assert_eq!(confirmed_of(&both, "not.asked.about"), None);
    let confirmed = confirmed_of(&both, &ntfy.package).expect("ntfy was asked about");
    confirm_distributor(data.path(), &confirmed).expect("kept");
    let kept = confirmed_distributor(data.path());
    assert_eq!(
        choose_distributor(kept.as_deref(), &both),
        DistributorChoice::Use(ntfy.clone())
    );

    // ntfy is gone and another distributor is there: keeper asks again.
    assert_eq!(
        choose_distributor(kept.as_deref(), std::slice::from_ref(&nextpush)),
        DistributorChoice::Ask(vec![nextpush.clone()])
    );

    // Settings offers each asked distributor by name and package, and its
    // status names the one whose server push reaches this device through.
    let asking = UnifiedPushVm::of(&UnifiedPushStatus::Asking(both.to_vec()), "tablet");
    assert_eq!(
        asking
            .offers
            .iter()
            .map(|offer| offer.package.as_str())
            .collect::<Vec<_>>(),
        [ntfy.package.as_str(), nextpush.package.as_str()]
    );
    assert!(asking.offers[0].confirm.contains("ntfy (io.heckel.ntfy)"));
    let registered = UnifiedPushVm::of(
        &UnifiedPushStatus::Registered {
            host: "ntfy.example".to_owned(),
            distributor: ntfy.clone(),
        },
        "tablet",
    );
    assert!(registered.offers.is_empty());
    assert!(registered.status.contains("ntfy (io.heckel.ntfy)"));
}

/// R98UP1-03: keeper asks an endpoint's server once — an endpoint it kept
/// whose server answered as a Matrix gateway is not asked again; a new
/// endpoint, or one whose server did not answer so, is.
#[test]
fn keeper_asks_an_endpoints_server_once() {
    let ntfy = endpoint(NTFY);
    let kept = |raw: &str, gateway: bool| KeptEndpoint {
        endpoint: endpoint(raw),
        distributor: distributor("io.heckel.ntfy", "ntfy"),
        gateway,
    };
    assert_eq!(
        known_gateway(Some(&kept(NTFY, true)), &ntfy).map(|g| g.as_str().to_owned()),
        Some("https://ntfy.example/_matrix/push/v1/notify".to_owned())
    );
    assert_eq!(known_gateway(None, &ntfy), None);
    assert_eq!(known_gateway(Some(&kept(NTFY, false)), &ntfy), None);
    assert_eq!(
        known_gateway(Some(&kept("https://ntfy.example/upOld?up=1", true)), &ntfy),
        None
    );

    // What keeper keeps survives a restart.
    let data = tempfile::tempdir().expect("a data directory");
    keep_endpoint(data.path(), Some(&kept(NTFY, true))).expect("kept");
    assert_eq!(kept_endpoint(data.path()), Some(kept(NTFY, true)));
    keep_endpoint(data.path(), None).expect("forgotten");
    assert_eq!(kept_endpoint(data.path()), None);
}

/// R98UP1-01: a server keeper could not reach is not an answer — the pusher
/// registered before stays; only a definite answer that it is not a Matrix
/// gateway (or an endpoint keeper refuses) removes it.
#[test]
fn an_unreachable_server_keeps_the_pusher_only_an_answer_removes_it() {
    let ntfy = endpoint(NTFY);
    let host = || "ntfy.example".to_owned();
    let unreachable = DiscoveryRefusal::Unreachable { host: host() };
    assert_eq!(
        pusher_change(Err(unreachable.clone())),
        PusherChange::Keep(unreachable)
    );
    for definite in [
        DiscoveryRefusal::Status {
            host: host(),
            status: 404,
        },
        DiscoveryRefusal::NotMatrix { host: host() },
        DiscoveryRefusal::OtherGateway {
            host: host(),
            kind: "webpush".to_owned(),
        },
        DiscoveryRefusal::Endpoint(EndpointRefusal::TooLong { host: host() }),
    ] {
        assert_eq!(
            pusher_change(Err(definite.clone())),
            PusherChange::Remove(definite)
        );
    }
    let gateway = gateway_of_answer(&ntfy, 200, br#"{"unifiedpush":{"gateway":"matrix"}}"#)
        .expect("a Matrix gateway");
    assert_eq!(
        pusher_change(Ok(gateway.clone())),
        PusherChange::Register(gateway)
    );
}

/// The mock shell's Android push status (`dev/unifiedpush-fixture.json`) is
/// what Rust's view model says, never a copy: this writes it when
/// `KEEPER_WRITE_FIXTURES` is set and otherwise fails on any drift.
#[test]
fn the_mock_shells_push_status_is_rusts() {
    let ntfy = distributor("io.heckel.ntfy", "ntfy");
    let fixture = serde_json::json!({
        "asking": UnifiedPushVm::of(&UnifiedPushStatus::Asking(vec![ntfy.clone()]), "tablet"),
        "registered": UnifiedPushVm::of(
            &UnifiedPushStatus::Registered { host: "ntfy.example.org".to_owned(), distributor: ntfy },
            "tablet",
        ),
    });
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../dev/unifiedpush-fixture.json");
    let written = format!(
        "{}\n",
        serde_json::to_string_pretty(&fixture).expect("json")
    );
    if std::env::var_os("KEEPER_WRITE_FIXTURES").is_some() {
        std::fs::write(&path, &written).expect("the fixture is written");
    }
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert_eq!(
        on_disk,
        written,
        "rerun with KEEPER_WRITE_FIXTURES=1 to rewrite {}",
        path.display()
    );
}

fn approval_shown(category: Option<Category>, tier: u8) -> Shown {
    Shown {
        title: "Nixi asks: run git push".to_owned(),
        subtitle: None,
        body: "T3 — [\"git\",\"push\"]".to_owned(),
        category,
        thread_id: ROOM.to_owned(),
        approval: Some(PushApproval {
            room: ROOM.to_owned(),
            id: "01APPROVAL".to_owned(),
            binding_digest: "d1".to_owned(),
            tier,
            category,
        }),
        answer: None,
    }
}

/// 98.4 #3, Q12: from Android 12 an approval posts with its category's
/// actions, and an action on what was posted decides exactly the record
/// shown — the iPhone's `action_decision` over Android's notification.
/// Before Android 12 the same approval posts with no category and no
/// actions, and its record carries none, so no action decides anything;
/// a tap still opens the session room.
#[test]
fn a_notification_offers_actions_only_where_android_can_require_unlock() {
    let shown = approval_shown(Some(Category::Approval), 3);
    let posted = phone_notification(ACCOUNT, ROOM, "$ask:example.org", &shown, 31);
    assert_eq!(posted.category.as_deref(), Some("KEEPER_APPROVAL"));
    assert_eq!(
        posted
            .actions
            .iter()
            .map(|a| (a.id.as_str(), a.destructive))
            .collect::<Vec<_>>(),
        [("KEEPER_APPROVE_ONCE", false), ("KEEPER_DENY", true)]
    );
    assert_eq!(posted.tag, "01APPROVAL");
    let tap = |posted: &PhoneNotification, action: &str| ActionTap {
        category: posted.category.clone().unwrap_or_default(),
        action: action.to_owned(),
        user_info: posted.user_info.clone(),
    };
    let decided = action_decision(&tap(&posted, "KEEPER_APPROVE_ONCE")).expect("offered");
    assert_eq!(decided.account, ACCOUNT);
    assert_eq!(decided.room, ROOM);
    assert_eq!(decided.req.id, "01APPROVAL");
    assert_eq!(decided.req.binding_digest, "d1");

    let old = phone_notification(ACCOUNT, ROOM, "$ask:example.org", &shown, 30);
    assert_eq!(old.category, None);
    assert!(old.actions.is_empty());
    for action in ["KEEPER_APPROVE_ONCE", "KEEPER_DENY"] {
        for category in ["", "KEEPER_APPROVAL"] {
            let forged = ActionTap {
                category: category.to_owned(),
                ..tap(&old, action)
            };
            assert_eq!(
                action_decision(&forged).map(|_| ()),
                Err(ActionRefusal::NotOffered),
                "{action} on {category:?}"
            );
        }
    }
    assert_eq!(
        tap_target(&old.user_info),
        Some(PushOpenVm::Room {
            account_id: ACCOUNT.to_owned(),
            room_id: ROOM.to_owned()
        })
    );
}

/// An answer is one notification its edits replace; an event that is
/// neither an approval nor an answer is its own.
#[test]
fn an_answer_is_one_notification_its_edits_replace() {
    let answer = Shown {
        title: "Nixi".to_owned(),
        subtitle: None,
        body: "Done.".to_owned(),
        category: None,
        thread_id: ROOM.to_owned(),
        approval: None,
        answer: Some("$anchor:example.org".to_owned()),
    };
    let first = phone_notification(ACCOUNT, ROOM, "$anchor:example.org", &answer, 34);
    let edit = phone_notification(ACCOUNT, ROOM, "$edit:example.org", &answer, 34);
    assert_eq!(first.tag, "$anchor:example.org");
    assert_eq!(edit.tag, first.tag);
    assert!(edit.actions.is_empty());
    let plain = Shown {
        answer: None,
        ..answer
    };
    assert_eq!(
        phone_notification(ACCOUNT, ROOM, "$other:example.org", &plain, 34).tag,
        "$other:example.org"
    );
}

/// A decision that could not be sent replaces its notification with one
/// that opens the same room and offers nothing; one that names no room
/// keeper composed opens nothing.
#[test]
fn an_unsent_decision_asks_the_person_to_finish_in_keeper() {
    let posted = phone_notification(
        ACCOUNT,
        ROOM,
        "$ask:example.org",
        &approval_shown(Some(Category::Approval), 3),
        34,
    );
    let finish = finish_in_keeper(&posted.tag, &posted.user_info, "The card is gone.");
    assert_eq!(finish.tag, posted.tag);
    assert!(finish.actions.is_empty() && finish.category.is_none());
    assert_eq!(finish.body, format!("{FINISH_IN_KEEPER} The card is gone."));
    assert_eq!(
        tap_target(&finish.user_info),
        Some(PushOpenVm::Room {
            account_id: ACCOUNT.to_owned(),
            room_id: ROOM.to_owned()
        })
    );
    let unknown = finish_in_keeper("t", &json!({"room_id": ROOM}), "");
    assert_eq!(unknown.body, FINISH_IN_KEEPER);
    assert_eq!(tap_target(&unknown.user_info), None);
}

/// 106.6 #6, #7: an agents server item posts one notification per item on
/// the tablet — a second delivery has the same tag and replaces it — with
/// actions only where Android requires unlock, and an unsent decision's
/// replacement still opens the item's session.
#[test]
fn an_agents_item_is_one_notification_and_its_replacement_opens_the_session() {
    use keeper_core::hq::push::{classify, HqClassified, ItemKind, PushedItem};

    let pushed = PushedItem {
        database: "neuradrive".to_owned(),
        session_id: "marta.1.2".to_owned(),
        kind: ItemKind::Approval,
        item_id: "01JAPPROVAL".to_owned(),
    };
    let row = json!({"id": "01JAPPROVAL", "session_id": "marta.1.2", "tool_call_id": "c1",
        "tier": "T1", "binding_digest": "d", "summary": "list files",
        "approvers": "[\"marta\"]", "state": "pending", "expires_at_ms": 2_000,
        "author_actor": "dixi"});
    let HqClassified::Show(shown) =
        classify(&pushed, row.as_object().expect("object"), "marta", 1_000)
    else {
        panic!("shown")
    };
    let first = item_notification(&shown, 34);
    let again = item_notification(&shown, 34);
    assert_eq!(first.tag, "neuradrive/approval/01JAPPROVAL");
    assert_eq!(again.tag, first.tag, "a second delivery replaces the first");
    assert_eq!(first.group, "neuradrive/marta.1.2");
    assert_eq!(first.category.as_deref(), Some("KEEPER_APPROVAL"));
    assert_eq!(first.actions.len(), 2);
    let old = item_notification(&shown, 30);
    assert!(old.actions.is_empty() && old.category.is_none());
    assert_eq!(old.user_info["keeper_hq_approval"]["category"], json!(null));

    let finish = finish_in_keeper(&first.tag, &first.user_info, "");
    assert_eq!(finish.group, "neuradrive/marta.1.2");
    assert_eq!(
        keeper_core::agents::push::open_target(&finish.user_info),
        Some(PushOpenVm::Session {
            database: "neuradrive".to_owned(),
            session_id: "marta.1.2".to_owned(),
        })
    );
    assert_eq!(finish.user_info.get("keeper_hq_approval"), None);
}
