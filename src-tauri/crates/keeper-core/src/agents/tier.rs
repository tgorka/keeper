//! How risky an agent's tool call is (AD-392, FR-796, FR-799).
//!
//! A pure rule over a closed table: the tool, facts about what the call
//! touches ([`CallFacts`]) and facts about the session that makes it
//! ([`Context`]) give one [`Classification`]. Nothing here asks a model,
//! reads a file or knows a host; the host gathers the facts and acts on the
//! answer — the row in `bot_audit`, the `tool_call` line's tier, and whether
//! the call runs, waits for a person or is refused.
//!
//! | tier | what | the host |
//! |---|---|---|
//! | T0 | reads | runs it |
//! | T1 | reversible inside the session, the surface, `delegate`, `reply`, `bmad_render`, `bmad_memlog` | runs it |
//! | T2 | a drive write outside the session, a first write the grant asks for | asks a person |
//! | T3 | a schedule or workflow set by an agent, a declassification | asks a person, once |
//! | T4 | (no tool of this build) | the requester decides |
//! | T5 | a write to the agent's own machine files or to `approvals/` | refuses |
//!
//! **The raise** (R171): a call that already needs a person (T2 and up) is
//! one tier stricter when the session is delegated, unattended or
//! `untrusted`, or the target is reached through a KVM — once, however many
//! of those hold. A T0 or T1 call is never raised: it stays automatic in
//! every session, and its row still names the reasons that held.

use serde_json::Value;

use crate::agents::label::Integrity;
use crate::agents::session::{SessionAgent, SessionKind};
use crate::bots::grant::GrantVerdict;
use crate::sessions::model::{ACTIVE_DIR, ARCHIVE_DIR};

/// What a call answers when its tier is T5: keeper never lets an agent do it.
pub const FORBIDDEN: &str = "keeper never lets an agent do this: it would change the agent's own configuration or the approvals that guard its work. A person can do it themselves. Nothing was changed.";

/// A session's folder of approval records, keeper's alone.
pub const APPROVALS_DIR: &str = "approvals";

/// A risk tier (AD-392). The derived order is the table's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// Observe.
    T0,
    /// Reversible and local.
    T1,
    /// A recoverable mutation.
    T2,
    /// External: it transmits, or a person's say over what runs.
    T3,
    /// Irreversible or privileged.
    T4,
    /// Forbidden.
    T5,
}

impl Tier {
    /// The number the log and the audit row store.
    pub fn as_u8(self) -> u8 {
        match self {
            Tier::T0 => 0,
            Tier::T1 => 1,
            Tier::T2 => 2,
            Tier::T3 => 3,
            Tier::T4 => 4,
            Tier::T5 => 5,
        }
    }

    /// The tier the number `n` stores, when it is one.
    pub fn from_u8(n: u8) -> Option<Tier> {
        [Tier::T0, Tier::T1, Tier::T2, Tier::T3, Tier::T4, Tier::T5]
            .into_iter()
            .find(|tier| tier.as_u8() == n)
    }

    /// One tier stricter; T5 is the ceiling.
    fn raised(self) -> Tier {
        match self {
            Tier::T0 => Tier::T1,
            Tier::T1 => Tier::T2,
            Tier::T2 => Tier::T3,
            Tier::T3 => Tier::T4,
            Tier::T4 | Tier::T5 => Tier::T5,
        }
    }
}

/// Every tool an agent's call can name in this build, and the action
/// `declassify`. Matched exhaustively, so a new tool has no tier until it
/// has a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentTool {
    DriveList,
    DriveRead,
    DriveGlob,
    DriveGrep,
    DriveStat,
    DriveWrite,
    DriveEdit,
    SessionWrite,
    CardUpdate,
    Delegate,
    Reply,
    SurfaceOpen,
    SurfaceHighlight,
    SurfacePoint,
    SurfaceScroll,
    SurfaceProposeEdit,
    BmadConfig,
    BmadRender,
    BmadMemlog,
    BmadParty,
    SkillsList,
    SkillView,
    Declassify,
}

impl AgentTool {
    /// Every tool, in the table's order.
    pub const ALL: [AgentTool; 23] = [
        AgentTool::DriveList,
        AgentTool::DriveRead,
        AgentTool::DriveGlob,
        AgentTool::DriveGrep,
        AgentTool::DriveStat,
        AgentTool::DriveWrite,
        AgentTool::DriveEdit,
        AgentTool::SessionWrite,
        AgentTool::CardUpdate,
        AgentTool::Delegate,
        AgentTool::Reply,
        AgentTool::SurfaceOpen,
        AgentTool::SurfaceHighlight,
        AgentTool::SurfacePoint,
        AgentTool::SurfaceScroll,
        AgentTool::SurfaceProposeEdit,
        AgentTool::BmadConfig,
        AgentTool::BmadRender,
        AgentTool::BmadMemlog,
        AgentTool::BmadParty,
        AgentTool::SkillsList,
        AgentTool::SkillView,
        AgentTool::Declassify,
    ];

    /// The name the model calls (the action's own word for `declassify`).
    pub fn as_wire(self) -> &'static str {
        match self {
            AgentTool::DriveList => "drive_list",
            AgentTool::DriveRead => "drive_read",
            AgentTool::DriveGlob => "drive_glob",
            AgentTool::DriveGrep => "drive_grep",
            AgentTool::DriveStat => "drive_stat",
            AgentTool::DriveWrite => "drive_write",
            AgentTool::DriveEdit => "drive_edit",
            AgentTool::SessionWrite => "session_write",
            AgentTool::CardUpdate => "card_update",
            AgentTool::Delegate => "delegate",
            AgentTool::Reply => "reply",
            AgentTool::SurfaceOpen => "surface_open",
            AgentTool::SurfaceHighlight => "surface_highlight",
            AgentTool::SurfacePoint => "surface_point",
            AgentTool::SurfaceScroll => "surface_scroll",
            AgentTool::SurfaceProposeEdit => "surface_propose_edit",
            AgentTool::BmadConfig => "bmad_config",
            AgentTool::BmadRender => "bmad_render",
            AgentTool::BmadMemlog => "bmad_memlog",
            AgentTool::BmadParty => "bmad_party",
            AgentTool::SkillsList => "skills_list",
            AgentTool::SkillView => "skill_view",
            AgentTool::Declassify => "declassify",
        }
    }

    /// The tool a name the model produced is; `None` has no row.
    pub fn from_wire(name: &str) -> Option<AgentTool> {
        AgentTool::ALL
            .into_iter()
            .find(|tool| tool.as_wire() == name)
    }
}

/// What a call touches, as far as its tier cares.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CallFacts {
    /// The target lies inside the calling session's own folder.
    pub in_session: bool,
    /// The card a `card_update` changes is another session's.
    pub other_session_card: bool,
    /// A card change, or a delegation's card, sets `schedule` or `workflow`
    /// (R28 S-21).
    pub sets_schedule: bool,
    /// A write to keeper's own files wherever it lands: an `agent.toml`, a
    /// `_drive.toml`, or anything in a session's `approvals/`.
    pub protected: bool,
}

/// The grant's answer for a drive verb, without its payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantWord {
    Allow,
    Ask,
    Deny,
}

impl From<&GrantVerdict> for GrantWord {
    fn from(verdict: &GrantVerdict) -> Self {
        match verdict {
            GrantVerdict::Allow { .. } => GrantWord::Allow,
            GrantVerdict::Ask { .. } => GrantWord::Ask,
            GrantVerdict::Deny { .. } => GrantWord::Deny,
        }
    }
}

/// What the session making the call says about it (R83).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    /// The work was handed on: a delegated session, or any hop ≥ 1.
    pub delegated: bool,
    /// Nobody watches it run: a scheduled or gate session (R83).
    pub unattended: bool,
    /// The session's label's integrity now.
    pub integrity: Integrity,
    /// The target is reached through a KVM (96.5; false until then).
    pub via_kvm: bool,
    /// The grant's answer, for a drive verb; `None` for every other tool.
    pub grant: Option<GrantWord>,
}

impl Context {
    /// The context of a call in the session `agent` describes, whose label
    /// is at `integrity` now. Unattended ⇔ the session kind is `scheduled`
    /// or `gate` (R83; a workflow's `checkpoints = "unattended"` joins
    /// them when 94.3 adds the key).
    pub fn of_session(agent: &SessionAgent, integrity: Integrity) -> Context {
        Context {
            delegated: agent.kind == SessionKind::Delegated || agent.hop >= 1,
            unattended: matches!(agent.kind, SessionKind::Scheduled | SessionKind::Gate),
            integrity,
            via_kvm: false,
            grant: None,
        }
    }
}

/// Why a call was raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Raise {
    Delegated,
    Unattended,
    Untrusted,
    Kvm,
}

impl Raise {
    /// The word the audit row and the record use.
    pub fn as_word(self) -> &'static str {
        match self {
            Raise::Delegated => "delegated",
            Raise::Unattended => "unattended",
            Raise::Untrusted => "untrusted",
            Raise::Kvm => "kvm",
        }
    }

    /// The raise `word` names, when it names one.
    pub fn from_word(word: &str) -> Option<Raise> {
        [
            Raise::Delegated,
            Raise::Unattended,
            Raise::Untrusted,
            Raise::Kvm,
        ]
        .into_iter()
        .find(|raise| raise.as_word() == word)
    }
}

/// What the host does with a classified call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// T0–T1: it runs.
    Run,
    /// T2–T4: a person decides first.
    Person,
    /// T5: never.
    Refuse,
}

/// One call's tier and how it got there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub tool: AgentTool,
    /// After the raise.
    pub tier: Tier,
    /// The row's tier, or the grant's floor when that is higher.
    pub base_tier: Tier,
    /// Every reason for a raise that held, in [`Raise`]'s order — also on a
    /// T0/T1 call, which the raise leaves where it is (R171).
    pub raised_by: Vec<Raise>,
}

impl Classification {
    /// Run, ask a person, or refuse.
    pub fn gate(&self) -> Gate {
        match self.tier {
            Tier::T0 | Tier::T1 => Gate::Run,
            Tier::T2 | Tier::T3 | Tier::T4 => Gate::Person,
            Tier::T5 => Gate::Refuse,
        }
    }

    /// `raised_by` as the audit row stores it: a comma list.
    pub fn raised_by_words(&self) -> String {
        self.raised_by
            .iter()
            .map(|raise| raise.as_word())
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// The table's row for `tool` over `facts` (AD-392 with epic 93's Q5, R81).
fn row(tool: AgentTool, facts: &CallFacts) -> Tier {
    match tool {
        AgentTool::DriveList
        | AgentTool::DriveRead
        | AgentTool::DriveGlob
        | AgentTool::DriveGrep
        | AgentTool::DriveStat
        | AgentTool::BmadConfig
        | AgentTool::BmadParty
        | AgentTool::SkillsList
        | AgentTool::SkillView => Tier::T0,
        AgentTool::DriveWrite
        | AgentTool::DriveEdit
        | AgentTool::SessionWrite
        | AgentTool::CardUpdate
            if facts.protected =>
        {
            Tier::T5
        }
        AgentTool::DriveWrite | AgentTool::DriveEdit if !facts.in_session => Tier::T2,
        AgentTool::DriveWrite | AgentTool::DriveEdit | AgentTool::SessionWrite => Tier::T1,
        AgentTool::CardUpdate | AgentTool::Delegate if facts.sets_schedule => Tier::T3,
        AgentTool::CardUpdate if facts.other_session_card => Tier::T2,
        AgentTool::CardUpdate | AgentTool::Delegate | AgentTool::Reply => Tier::T1,
        AgentTool::SurfaceOpen
        | AgentTool::SurfaceHighlight
        | AgentTool::SurfacePoint
        | AgentTool::SurfaceScroll
        | AgentTool::SurfaceProposeEdit => Tier::T1,
        // A render published into the session's workspace/, a memlog in its
        // artifacts/: both land inside the session by construction (R105).
        AgentTool::BmadRender | AgentTool::BmadMemlog => Tier::T1,
        AgentTool::Declassify => Tier::T3,
    }
}

/// Classify one call (AD-392, R171): its row, at least T2 when the grant
/// asks (the higher wins), then raised once when it needs a person already
/// and any reason holds.
pub fn classify(tool: AgentTool, facts: &CallFacts, context: &Context) -> Classification {
    let grant_floor = match context.grant {
        Some(GrantWord::Ask) => Tier::T2,
        _ => Tier::T0,
    };
    let base_tier = row(tool, facts).max(grant_floor);
    let raised_by: Vec<Raise> = [
        (context.delegated, Raise::Delegated),
        (context.unattended, Raise::Unattended),
        (context.integrity <= Integrity::Untrusted, Raise::Untrusted),
        (context.via_kvm, Raise::Kvm),
    ]
    .into_iter()
    .filter_map(|(holds, raise)| holds.then_some(raise))
    .collect();
    let tier = if base_tier >= Tier::T2 && !raised_by.is_empty() {
        base_tier.raised()
    } else {
        base_tier
    };
    Classification {
        tool,
        tier,
        base_tier,
        raised_by,
    }
}

/// Where the calling session lives: its home drive, and its folder there
/// as keeper-sync lands it — drive-relative names, every link followed.
#[derive(Debug, Clone, Copy)]
pub struct Place<'a> {
    pub home_drive: &'a str,
    pub session_dir: &'a [String],
}

/// Whether two names are one entry on the volume the host writes to: the
/// host passes keeper-sync's folded comparison, so `Approvals/` and
/// `AGENT.toml` are what the Mac makes of them.
pub type SameName = dyn Fn(&str, &str) -> bool;

/// Whether `landed` is one of keeper's own files, on any drive: an
/// `agent.toml`, a `_drive.toml`, or anything inside a session's
/// `approvals/` (`active/<session>/approvals/…` or
/// `archive/<year>/<session>/approvals/…`, under whichever zone).
fn keepers(landed: &[String], same: &SameName) -> bool {
    let named = |at: usize, name: &str| landed.get(at).is_some_and(|part| same(part, name));
    let machine = landed.last().is_some_and(|last| {
        same(last, crate::agents::home::FILE_NAME) || same(last, crate::agents::drive::FILE_NAME)
    });
    machine
        || (0..landed.len()).any(|at| {
            named(at, APPROVALS_DIR)
                && ((at >= 2 && named(at - 2, ACTIVE_DIR))
                    || (at >= 3 && named(at - 3, ARCHIVE_DIR)))
        })
}

/// The facts of a write that lands at `landed` — drive-relative names, as
/// keeper-sync's landing resolves the requested path — in `drive`, for a
/// session at `place`. What the call spelled is never consulted: a link or
/// another case of a name lands where the executor would land it.
pub fn landed_facts(
    drive: &str,
    landed: &[String],
    place: &Place<'_>,
    same: &SameName,
) -> CallFacts {
    let in_session = drive == place.home_drive
        && landed.len() > place.session_dir.len()
        && landed
            .iter()
            .zip(place.session_dir)
            .all(|(part, dir)| same(part, dir));
    CallFacts {
        in_session,
        protected: keepers(landed, same),
        ..CallFacts::default()
    }
}

/// The facts of a session tool's or a delegation's call: `landed`, the
/// [`landed_facts`] of the file a `session_write` or `card_update` lands
/// at (a card that lands outside this session is another session's);
/// and from its arguments, whether a `card_update` or a new delegation's
/// card names `schedule` or `workflow`.
pub fn named_facts(tool: AgentTool, args: &Value, landed: CallFacts) -> CallFacts {
    let schedules = |object: &Value| {
        object
            .as_object()
            .is_some_and(|keys| keys.contains_key("schedule") || keys.contains_key("workflow"))
    };
    match tool {
        AgentTool::SessionWrite => CallFacts {
            in_session: landed.in_session,
            protected: landed.protected,
            ..CallFacts::default()
        },
        AgentTool::CardUpdate => CallFacts {
            in_session: landed.in_session,
            other_session_card: !landed.in_session,
            sets_schedule: schedules(&args["fields"]),
            protected: landed.protected,
        },
        // A next round's card is dropped (R49), so it sets nothing.
        AgentTool::Delegate => CallFacts {
            sets_schedule: args["session"].is_null() && schedules(&args["card"]),
            ..CallFacts::default()
        },
        _ => CallFacts::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attended() -> Context {
        Context {
            delegated: false,
            unattended: false,
            integrity: Integrity::Owner,
            via_kvm: false,
            grant: None,
        }
    }

    fn tier(tool: AgentTool, facts: CallFacts) -> Tier {
        classify(tool, &facts, &attended()).tier
    }

    const SESSION_DIR: &str = "60-sessions/active/2026-10-05-inbox";

    /// A folded comparison standing in for keeper-sync's.
    fn same(a: &str, b: &str) -> bool {
        a.eq_ignore_ascii_case(b)
    }

    fn names(path: &str) -> Vec<String> {
        path.split('/').map(str::to_owned).collect()
    }

    /// The facts of a write that lands at `path` of `drive`.
    fn at(drive: &str, path: &str) -> CallFacts {
        let session_dir = names(SESSION_DIR);
        let place = Place {
            home_drive: "tgdrive",
            session_dir: &session_dir,
        };
        landed_facts(drive, &names(path), &place, &same)
    }

    /// The landing facts of a session tool's target, session-relative.
    fn in_session(rel: &str) -> CallFacts {
        at("tgdrive", &format!("{SESSION_DIR}/{rel}"))
    }

    /// 93.1 acceptance 1: every tool has its row, read off the facts a host
    /// gathers. The match is exhaustive, so a new tool fails to compile
    /// until it is added here.
    #[test]
    fn every_tool_has_a_tier_row() {
        let outside = at("tgdrive", "10-notes/a.md");
        let schedule = named_facts(
            AgentTool::CardUpdate,
            &serde_json::json!({"card": "card.md", "fields": {"schedule": "@daily"}}),
            in_session("card.md"),
        );
        for tool in AgentTool::ALL {
            assert_eq!(AgentTool::from_wire(tool.as_wire()), Some(tool));
            match tool {
                AgentTool::DriveList
                | AgentTool::DriveRead
                | AgentTool::DriveGlob
                | AgentTool::DriveGrep
                | AgentTool::DriveStat
                | AgentTool::BmadConfig
                | AgentTool::BmadParty
                | AgentTool::SkillsList
                | AgentTool::SkillView => {
                    assert_eq!(tier(tool, outside), Tier::T0, "{tool:?}");
                }
                AgentTool::DriveWrite | AgentTool::DriveEdit => {
                    assert_eq!(tier(tool, outside), Tier::T2);
                    assert_eq!(tier(tool, at("neuradrive", "10-notes/a.md")), Tier::T2);
                    assert_eq!(
                        tier(
                            tool,
                            at(
                                "tgdrive",
                                "60-sessions/active/2026-10-05-inbox/workspace/x.md"
                            )
                        ),
                        Tier::T1
                    );
                    // Another session's folder is outside this one.
                    assert_eq!(
                        tier(
                            tool,
                            at("tgdrive", "60-sessions/active/2026-10-04-other/x.md")
                        ),
                        Tier::T2
                    );
                    // keeper's own files wherever they land, in any case,
                    // on any drive.
                    for keepers in [
                        "80-agents/nixi/agent.toml",
                        "80-agents/nixi/Agent.TOML",
                        "80-agents/_drive.toml",
                        "80-agents/_DRIVE.toml",
                        "60-sessions/active/2026-10-05-inbox/approvals/01J.json",
                        "60-sessions/active/2026-10-05-inbox/Approvals/01J.json",
                        "60-sessions/Active/2026-10-04-other/approvals/01J.decision.json",
                        "60-sessions/archive/2026/2026-01-02-old/approvals/01J.json",
                    ] {
                        assert_eq!(tier(tool, at("tgdrive", keepers)), Tier::T5, "{keepers}");
                        assert_eq!(tier(tool, at("neuradrive", keepers)), Tier::T5, "{keepers}");
                    }
                    // `approvals` outside a session's folder is an ordinary
                    // folder.
                    for plain in [
                        "10-notes/approvals/x.md",
                        "60-sessions/active/approvals/x.md",
                    ] {
                        assert_eq!(tier(tool, at("tgdrive", plain)), Tier::T2, "{plain}");
                    }
                    // This session's folder in another case is still it.
                    assert_eq!(
                        tier(
                            tool,
                            at("tgdrive", "60-Sessions/active/2026-10-05-INBOX/x.md")
                        ),
                        Tier::T1
                    );
                    assert_eq!(
                        tier(tool, at("neuradrive", &format!("{SESSION_DIR}/x.md"))),
                        Tier::T2
                    );
                }
                AgentTool::SessionWrite => {
                    let write = |path: &str| {
                        tier(
                            tool,
                            named_facts(
                                tool,
                                &serde_json::json!({"path": path, "content": "x"}),
                                in_session(path),
                            ),
                        )
                    };
                    assert_eq!(write("notes.md"), Tier::T1);
                    assert_eq!(write("workspace/x.csv"), Tier::T1);
                    assert_eq!(write("approvals/01J.json"), Tier::T5);
                    assert_eq!(write("APPROVALS/01J.json"), Tier::T5);
                }
                AgentTool::CardUpdate => {
                    let status = named_facts(
                        tool,
                        &serde_json::json!({"card": "card.md", "fields": {"status": "done"}}),
                        in_session("card.md"),
                    );
                    assert_eq!(tier(tool, status), Tier::T1);
                    // A card that lands in another session.
                    let other = named_facts(
                        tool,
                        &serde_json::json!({"card": "card.md", "fields": {"status": "done"}}),
                        at("tgdrive", "60-sessions/active/2026-10-04-other/card.md"),
                    );
                    assert_eq!(tier(tool, other), Tier::T2);
                    let approvals = named_facts(
                        tool,
                        &serde_json::json!({"card": "approvals/x.md", "fields": {"status": "done"}}),
                        in_session("approvals/x.md"),
                    );
                    assert_eq!(tier(tool, approvals), Tier::T5);
                    assert_eq!(tier(tool, schedule), Tier::T3);
                    let workflow = named_facts(
                        tool,
                        &serde_json::json!({"card": "card.md", "fields": {"workflow": "triage"}}),
                        in_session("card.md"),
                    );
                    assert_eq!(tier(tool, workflow), Tier::T3);
                    // At every session kind, the person's own DM included.
                    let main = Context {
                        integrity: Integrity::Owner,
                        ..attended()
                    };
                    assert_eq!(classify(tool, &schedule, &main).tier, Tier::T3);
                }
                AgentTool::Delegate => {
                    let plain = named_facts(
                        tool,
                        &serde_json::json!({"agent": "x/y", "brief": "b"}),
                        CallFacts::default(),
                    );
                    assert_eq!(tier(tool, plain), Tier::T1);
                    let carded = named_facts(
                        tool,
                        &serde_json::json!({"agent": "x/y", "brief": "b", "card": {"title": "t", "workflow": "w"}}),
                        CallFacts::default(),
                    );
                    assert_eq!(tier(tool, carded), Tier::T3);
                    // A next round drops its card (R49).
                    let round = named_facts(
                        tool,
                        &serde_json::json!({"session": "01J", "brief": "b", "card": {"schedule": "@daily"}}),
                        CallFacts::default(),
                    );
                    assert_eq!(tier(tool, round), Tier::T1);
                }
                AgentTool::Reply => {
                    assert_eq!(
                        tier(
                            tool,
                            named_facts(
                                tool,
                                &serde_json::json!({"text": "x"}),
                                CallFacts::default()
                            )
                        ),
                        Tier::T1
                    );
                }
                AgentTool::SurfaceOpen
                | AgentTool::SurfaceHighlight
                | AgentTool::SurfacePoint
                | AgentTool::SurfaceScroll
                | AgentTool::SurfaceProposeEdit
                | AgentTool::BmadRender
                | AgentTool::BmadMemlog => {
                    assert_eq!(tier(tool, CallFacts::default()), Tier::T1, "{tool:?}");
                }
                AgentTool::Declassify => {
                    assert_eq!(tier(tool, CallFacts::default()), Tier::T3);
                }
            }
        }
        assert_eq!(AgentTool::from_wire("run"), None);
        // A first write the grant asks for is at least T2; a read the grant
        // allows stays T0, and a T5 stays T5 whatever the grant says.
        let asks = Context {
            grant: Some(GrantWord::Ask),
            ..attended()
        };
        let inside = at("tgdrive", "60-sessions/active/2026-10-05-inbox/x.md");
        assert_eq!(
            classify(AgentTool::DriveWrite, &inside, &asks).tier,
            Tier::T2
        );
        assert_eq!(
            classify(AgentTool::DriveWrite, &inside, &asks).base_tier,
            Tier::T2
        );
        let allows = Context {
            grant: Some(GrantWord::Allow),
            ..attended()
        };
        assert_eq!(
            classify(AgentTool::DriveRead, &outside, &allows).tier,
            Tier::T0
        );
        assert_eq!(
            classify(AgentTool::DriveWrite, &inside, &allows).tier,
            Tier::T1
        );
        let machine = at("tgdrive", "80-agents/nixi/agent.toml");
        assert_eq!(
            classify(AgentTool::DriveWrite, &machine, &asks).tier,
            Tier::T5
        );
    }

    /// 93.1 acceptance 2, as R171 reads it: one tier however many reasons,
    /// only for a call that needs a person already; T4 raised is T5; T5
    /// stays T5; a T0/T1 call stays where it is and names its reasons.
    #[test]
    fn the_raise_is_one_tier_however_many_reasons() {
        let write = at("tgdrive", "10-notes/a.md");
        let delegated = Context {
            delegated: true,
            ..attended()
        };
        let once = classify(AgentTool::DriveWrite, &write, &delegated);
        assert_eq!((once.base_tier, once.tier), (Tier::T2, Tier::T3));
        assert_eq!(once.raised_by, vec![Raise::Delegated]);
        for (context, raise) in [
            (
                Context {
                    unattended: true,
                    ..attended()
                },
                Raise::Unattended,
            ),
            (
                Context {
                    integrity: Integrity::Untrusted,
                    ..attended()
                },
                Raise::Untrusted,
            ),
            (
                Context {
                    via_kvm: true,
                    ..attended()
                },
                Raise::Kvm,
            ),
        ] {
            let alone = classify(AgentTool::DriveWrite, &write, &context);
            assert_eq!(alone.tier, Tier::T3, "{raise:?}");
            assert_eq!(alone.raised_by, vec![raise]);
        }
        // `agent` and `peer` integrity are not outside content.
        for integrity in [Integrity::Agent, Integrity::Peer] {
            let trusted = Context {
                integrity,
                ..attended()
            };
            assert_eq!(
                classify(AgentTool::DriveWrite, &write, &trusted).tier,
                Tier::T2
            );
        }
        let all = Context {
            delegated: true,
            unattended: true,
            integrity: Integrity::Untrusted,
            via_kvm: true,
            grant: Some(GrantWord::Ask),
        };
        let four = classify(AgentTool::DriveWrite, &write, &all);
        assert_eq!((four.base_tier, four.tier), (Tier::T2, Tier::T3));
        assert_eq!(
            four.raised_by,
            vec![
                Raise::Delegated,
                Raise::Unattended,
                Raise::Untrusted,
                Raise::Kvm
            ]
        );
        assert_eq!(four.raised_by_words(), "delegated,unattended,untrusted,kvm");
        let schedule = CallFacts {
            sets_schedule: true,
            ..CallFacts::default()
        };
        assert_eq!(
            classify(AgentTool::CardUpdate, &schedule, &all).tier,
            Tier::T4
        );
        assert_eq!(Tier::T4.raised(), Tier::T5);
        let machine = at("tgdrive", "80-agents/nixi/agent.toml");
        let forbidden = classify(AgentTool::DriveWrite, &machine, &all);
        assert_eq!((forbidden.base_tier, forbidden.tier), (Tier::T5, Tier::T5));
        assert_eq!(forbidden.gate(), Gate::Refuse);
        // R171: T0 and T1 are never raised, and their reasons are kept.
        let watched_by_nobody = Context { grant: None, ..all };
        let reply = classify(AgentTool::Reply, &CallFacts::default(), &watched_by_nobody);
        assert_eq!((reply.base_tier, reply.tier), (Tier::T1, Tier::T1));
        assert_eq!(reply.raised_by.len(), 4);
        assert_eq!(reply.gate(), Gate::Run);
        let read = classify(AgentTool::DriveRead, &write, &watched_by_nobody);
        assert_eq!(read.tier, Tier::T0);
        assert_eq!(once.gate(), Gate::Person);
    }

    /// R83: unattended is the session kind; delegated is the kind or a hop.
    #[test]
    fn the_context_comes_from_the_session() {
        let text = include_str!(
            "../../tests/fixtures/agents/sessions/active/2026-09-30-release-notes/agent.toml"
        );
        let mut agent = crate::agents::session::parse_session_agent_toml(text).expect("agent.toml");
        for kind in SessionKind::ALL {
            agent.kind = kind;
            agent.hop = 0;
            let context = Context::of_session(&agent, Integrity::Agent);
            assert_eq!(
                context.delegated,
                kind == SessionKind::Delegated,
                "{kind:?}"
            );
            assert_eq!(
                context.unattended,
                matches!(kind, SessionKind::Scheduled | SessionKind::Gate),
                "{kind:?}"
            );
            assert!(!context.via_kvm);
            assert_eq!(context.grant, None);
            agent.hop = 1;
            assert!(Context::of_session(&agent, Integrity::Agent).delegated);
        }
    }
}
