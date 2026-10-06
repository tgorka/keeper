//! Prompt composition in AD-363's fixed order, and what the agent was told (story 89.3).
//!
//! [`compose`] is pure and orders the system message in six slots: (1) the
//! soul, (2) the frozen core memory, (3) the skills by name and description,
//! (4) the menu, (5) the session frame, ending in [`FILE_CONTENT_IS_DATA`],
//! and (6) the context files under the untrusted preamble. Every host composes
//! the same bytes from the same files, and the digest of those bytes is what
//! the session's `open` line records.
//!
//! [`told`] is the same text cut at the slot boundaries, so "what the agent
//! was told" and what the model received cannot differ.

use std::fmt::Write as _;
use std::ops::Range;

use chrono::{DateTime, FixedOffset, SecondsFormat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use super::home::{MenuAction, MenuItem};
use super::memory::{MemorySnapshot, SEPARATOR};
use super::skills::SkillsIndex;
use super::soul::Soul;
use crate::agents::events::Focus;
use crate::bots::context_files::ContextBundle;
use crate::bots::tools::FILE_CONTENT_IS_DATA;

/// Where and when a session runs: slot 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFrame {
    /// The agent id.
    pub agent: String,
    /// The writing host's slug.
    pub host: String,
    /// The session folder, drive-relative.
    pub session_path: String,
    pub session_kind: String,
    /// `(drive id, title)` of every drive in scope, the home drive first.
    pub drives: Vec<(String, String)>,
    /// The session label's sentence (`Label::sentence`): who may see what is
    /// read here.
    pub audience_sentence: String,
    /// The writing host's local time, with its offset (choice C4).
    pub now: DateTime<FixedOffset>,
    /// The note the person's docked notes view shows, when its drive is in
    /// scope (R41): held in memory, never logged.
    pub focus: Option<Focus>,
    /// What BMAD may assume here, for a turn offered a tool through which
    /// it follows a BMAD skill or workflow
    /// ([`super::workflow::frame_lines`]); empty otherwise.
    pub bmad: Vec<String>,
}

/// One persistent fact as it enters the prompt: a sentence, or a file from
/// the agent's home that the host read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderedFact {
    Text(String),
    File { path: String, text: String },
}

/// Everything one composition reads.
#[derive(Debug, Clone, Copy)]
pub struct PromptInput<'a> {
    pub soul: &'a Soul,
    pub facts: &'a [RenderedFact],
    pub memory: &'a MemorySnapshot,
    pub skills: &'a SkillsIndex,
    pub menu: &'a [MenuItem],
    pub frame: &'a SessionFrame,
    pub context: Option<&'a ContextBundle>,
}

/// One slot's span of the composed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSection {
    /// 1–6, AD-363's order.
    pub slot: u8,
    pub title: &'static str,
    pub range: Range<usize>,
}

/// The system message, its slots and its digests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedPrompt {
    pub text: String,
    /// Contiguous: concatenated, they are `text`.
    pub sections: Vec<PromptSection>,
    /// SHA-256 of `text`'s UTF-8, lowercase hex.
    pub prompt_sha256: String,
    /// The memory snapshot's digest.
    pub memory_sha256: String,
    /// What was found and left out: memory problems, refused skills, skill
    /// warnings, skipped context files.
    pub notes: Vec<String>,
}

const SOUL: &str = "Who you are";
const MEMORY: &str = "What you remember";
const SKILLS: &str = "Skills you can load";
const MENU: &str = "Menu";
const FRAME: &str = "This session";
const CONTEXT: &str = "Context files";

struct Builder {
    text: String,
    sections: Vec<PromptSection>,
}

impl Builder {
    /// Open a slot. The blank line between two slots belongs to the earlier
    /// one, so the sections stay contiguous.
    fn open(&mut self, slot: u8, title: &'static str) {
        if let Some(last) = self.sections.last_mut() {
            self.text.push('\n');
            last.range.end = self.text.len();
        }
        let start = self.text.len();
        let _ = writeln!(self.text, "# {title}\n");
        self.sections.push(PromptSection {
            slot,
            title,
            range: start..start,
        });
    }

    fn close(&mut self) {
        if let Some(last) = self.sections.last_mut() {
            last.range.end = self.text.len();
        }
    }

    /// A paragraph: the text, then a newline.
    fn line(&mut self, text: &str) {
        self.text.push_str(text);
        self.text.push('\n');
        self.close();
    }
}

/// Compose the system message.
pub fn compose(input: &PromptInput<'_>) -> ComposedPrompt {
    let mut b = Builder {
        text: String::new(),
        sections: Vec::new(),
    };
    let mut notes = Vec::new();

    soul_slot(&mut b, input.soul, input.facts);
    memory_slot(&mut b, input.memory, &mut notes);

    for (dir, reasons) in &input.skills.refused {
        notes.push(format!(
            "_skills/{dir} is not offered: {}",
            reasons.join(" ")
        ));
    }
    notes.extend(input.skills.warnings.iter().cloned());
    if !input.skills.offered.is_empty() {
        b.open(3, SKILLS);
        b.line(
            "Each skill's instructions load with skill_view; only its name and purpose are here.",
        );
        b.text.push('\n');
        for skill in &input.skills.offered {
            b.line(&format!("- {}: {}", skill.name, skill.description));
        }
    }

    if !input.menu.is_empty() {
        b.open(4, MENU);
        for item in input.menu {
            let runs = match &item.action {
                MenuAction::Workflow(workflow) => format!("runs the workflow {workflow}"),
                MenuAction::Prompt(_) => "runs a prompt".to_owned(),
            };
            b.line(&format!("- {}: {} ({runs})", item.code, item.description));
        }
    }

    frame_slot(&mut b, input.frame, input.facts);

    if let Some(bundle) = input.context {
        for skip in &bundle.skipped {
            notes.push(skip.sentence());
        }
        if let Some(block) = bundle.system_prompt() {
            b.open(6, CONTEXT);
            b.line(&block);
        }
    }

    let prompt_sha256 = hex::encode(Sha256::digest(b.text.as_bytes()));
    ComposedPrompt {
        text: b.text,
        sections: b.sections,
        prompt_sha256,
        memory_sha256: input.memory.sha256.clone(),
        notes,
    }
}

/// The session frame alone — slot 5 as [`compose`] renders it, without
/// the home's files: what a helper is told of the session it works for,
/// with no soul and no core memory (AD-399).
pub fn frame_text(frame: &SessionFrame) -> String {
    let mut b = Builder {
        text: String::new(),
        sections: Vec::new(),
    };
    frame_slot(&mut b, frame, &[]);
    b.text
}

fn soul_slot(b: &mut Builder, soul: &Soul, facts: &[RenderedFact]) {
    b.open(1, SOUL);
    b.line(&format!("Name: {}", soul.name));
    b.line(&format!("Title: {}", soul.title));
    if !soul.icon.is_empty() {
        b.line(&format!("Icon: {}", soul.icon));
    }
    b.line(&format!("Role: {}", soul.role));
    for (title, text) in [
        ("Identity", soul.identity.as_str()),
        ("Communication style", soul.communication_style.as_str()),
    ] {
        b.text.push('\n');
        b.line(&format!("## {title}\n"));
        b.line(text);
    }
    if !soul.principles.is_empty() {
        b.text.push('\n');
        b.line("## Principles\n");
        for principle in &soul.principles {
            b.line(&format!("- {principle}"));
        }
    }
    // A `file:` fact's file is content, not the person's sentence: it is
    // given after the frame's data sentence (slot 5), never here.
    let sentences: Vec<&str> = facts
        .iter()
        .filter_map(|fact| match fact {
            RenderedFact::Text(text) => Some(text.as_str()),
            RenderedFact::File { .. } => None,
        })
        .collect();
    if !sentences.is_empty() {
        b.text.push('\n');
        b.line("## Persistent facts\n");
        for text in sentences {
            b.line(&format!("- {text}"));
        }
    }
    let body = soul.body.trim();
    if !body.is_empty() {
        b.text.push('\n');
        b.line(body);
    }
}

fn memory_slot(b: &mut Builder, memory: &MemorySnapshot, notes: &mut Vec<String>) {
    b.open(2, MEMORY);
    for (title, file, entries) in [
        ("About your people", "USER.md", &memory.user),
        ("About the work", "MEMORY.md", &memory.memory),
    ] {
        if !b.text.ends_with("\n\n") {
            b.text.push('\n');
        }
        b.line(&format!("## {title} ({file})\n"));
        let problem = memory.problems.iter().find(|p| p.file == file);
        match problem {
            Some(problem) => {
                notes.push(problem.sentence.clone());
                b.line(&format!("Left out of this session: {}", problem.sentence));
            }
            None if entries.is_empty() => b.line("Nothing yet."),
            None => {
                let joined: Vec<&str> = entries.iter().map(String::as_str).collect();
                b.line(&joined.join(SEPARATOR));
            }
        }
    }
}

fn frame_slot(b: &mut Builder, frame: &SessionFrame, facts: &[RenderedFact]) {
    b.open(5, FRAME);
    b.line(&format!("You are {}@{}.", frame.agent, frame.host));
    b.line(&format!(
        "Session: {} ({}).",
        frame.session_path, frame.session_kind
    ));
    b.line("Drives in scope:");
    for (id, title) in &frame.drives {
        b.line(&format!("- {id}: {title}"));
    }
    b.line(&frame.audience_sentence);
    if let Some(focus) = &frame.focus {
        let under = focus
            .heading
            .as_ref()
            .map(|heading| format!(", under the heading {heading}"))
            .unwrap_or_default();
        b.line(&format!(
            "The person is looking at {} in {}{under}.",
            focus.path, focus.drive
        ));
    }
    b.line(&format!(
        "Now: {}",
        frame.now.to_rfc3339_opts(SecondsFormat::Secs, false)
    ));
    if !frame.bmad.is_empty() {
        b.text.push('\n');
        for line in &frame.bmad {
            b.line(line);
        }
    }
    b.text.push('\n');
    b.line(FILE_CONTENT_IS_DATA);
    for fact in facts {
        if let RenderedFact::File { path, text } = fact {
            b.text.push('\n');
            b.line(&format!("--- home file: {path} ---"));
            b.line(text.trim_end());
        }
    }
}

/// One slot of "what the agent was told".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ToldSectionVm {
    /// 1–6, AD-363's order.
    pub slot: u8,
    pub title: String,
    /// The slot's text exactly as the model received it.
    pub text: String,
}

/// What the agent was told: the composed system message by slot, its
/// digests, and what was left out. Shown by `keeper-agentd status --session`
/// (90.5) and the agent room (91.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentToldVm {
    pub sections: Vec<ToldSectionVm>,
    pub prompt_sha256: String,
    pub memory_sha256: String,
    pub notes: Vec<String>,
}

/// The composed prompt as the person reads it.
pub fn told(prompt: &ComposedPrompt) -> AgentToldVm {
    AgentToldVm {
        sections: prompt
            .sections
            .iter()
            .map(|section| ToldSectionVm {
                slot: section.slot,
                title: section.title.to_owned(),
                text: prompt.text[section.range.clone()].to_owned(),
            })
            .collect(),
        prompt_sha256: prompt.prompt_sha256.clone(),
        memory_sha256: prompt.memory_sha256.clone(),
        notes: prompt.notes.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::label::{Integrity, Label};
    use crate::agents::{drive, home, memory, skills, soul};
    use crate::bots::context_files::ContextFile;

    fn fixture(path: &str) -> String {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/agents/");
        std::fs::read_to_string(format!("{root}{path}")).expect("fixture is readable")
    }

    struct Home {
        soul: Soul,
        facts: Vec<RenderedFact>,
        memory: MemorySnapshot,
        skills: SkillsIndex,
        menu: Vec<MenuItem>,
        frame: SessionFrame,
        context: ContextBundle,
    }

    /// Nixi's fixture home, read as the host reads it.
    fn nixi() -> Home {
        let drive = drive::parse(&fixture("zone-ok/_drive.toml")).expect("_drive.toml");
        let config = home::parse_agent_toml(&fixture("zone-ok/nixi/agent.toml"), "nixi", &drive)
            .expect("agent.toml");
        let soul =
            soul::parse_soul(&fixture("zone-ok/nixi/SOUL.md"), &config.name).expect("SOUL.md");
        let facts = soul
            .persistent_facts
            .iter()
            .map(|fact| match fact {
                soul::Fact::Text(text) => RenderedFact::Text(text.clone()),
                soul::Fact::File(path) => RenderedFact::File {
                    path: path.clone(),
                    text: fixture(&format!("zone-ok/nixi/{path}")),
                },
            })
            .collect();
        let memory = memory::snapshot(
            Some(&fixture("zone-ok/nixi/USER.md")),
            Some(&fixture("zone-ok/nixi/MEMORY.md")),
        );
        let skill_dirs = [
            "My_Skill",
            "inbox-triage",
            "mismatch",
            "okf-note",
            "weekly-review",
        ];
        let found: Vec<(String, String)> = skill_dirs
            .iter()
            .map(|dir| {
                (
                    (*dir).to_owned(),
                    fixture(&format!("zone-ok/_skills/{dir}/SKILL.md")),
                )
            })
            .collect();
        let skills = skills::index(&found, &skills::SkillFilter::from_list(&config.skills));
        let label = Label::opening(&drive, Integrity::Owner);
        let frame = SessionFrame {
            agent: config.id.clone(),
            host: "electra".to_owned(),
            session_path: "60-sessions/active/2026-10-02-morning-inbox".to_owned(),
            session_kind: "main".to_owned(),
            drives: vec![("tgdrive".to_owned(), "tgdrive".to_owned())],
            audience_sentence: label.sentence(&|user| user.localpart().to_owned()),
            now: DateTime::parse_from_rfc3339("2026-10-02T10:15:03+02:00").expect("time"),
            focus: None,
            bmad: Vec::new(),
        };
        let agents_md = fixture("zone-ok/AGENTS.md");
        let bytes = agents_md.len();
        let context = ContextBundle {
            files: vec![ContextFile {
                subpath: "80-agents/AGENTS.md".to_owned(),
                bytes: bytes as u64,
                of_bytes: bytes as u64,
                text: agents_md,
                truncated: false,
            }],
            skipped: Vec::new(),
            total_bytes: bytes,
        };
        Home {
            soul,
            facts,
            memory,
            skills,
            menu: config.menu,
            frame,
            context,
        }
    }

    fn compose_home(home: &Home) -> ComposedPrompt {
        compose(&PromptInput {
            soul: &home.soul,
            facts: &home.facts,
            memory: &home.memory,
            skills: &home.skills,
            menu: &home.menu,
            frame: &home.frame,
            context: Some(&home.context),
        })
    }

    #[test]
    fn a_home_composes_to_the_golden_prompt() {
        let prompt = compose_home(&nixi());
        assert_eq!(prompt.text, fixture("nixi-told.md"));

        let slots: Vec<u8> = prompt.sections.iter().map(|s| s.slot).collect();
        assert_eq!(slots, [1, 2, 3, 4, 5, 6]);
        let slot = |n: u8| {
            let section = prompt.sections.iter().find(|s| s.slot == n).expect("slot");
            &prompt.text[section.range.clone()]
        };
        assert!(slot(1).starts_with("# Who you are\n\nName: Nixi\n"));
        assert!(
            slot(2).contains("(USER.md)")
                && slot(2).find("(USER.md)") < slot(2).find("(MEMORY.md)")
        );
        assert!(slot(3).contains("- inbox-triage: Read what arrived"));
        assert!(
            !slot(3).contains("drive_list"),
            "a skill's body is not in the prompt"
        );
        assert!(slot(4).contains("- TR: Triage what came in today"));
        assert!(slot(5).contains("You are nixi@electra."));
        assert!(slot(5).contains("What you read here may be shown only to: tgorka."));
        // A `file:` fact is data: after the data sentence, never in the soul.
        let data_at = slot(5)
            .find(FILE_CONTENT_IS_DATA)
            .expect("the data sentence");
        let file_at = slot(5)
            .find("--- home file: notes/standing-orders.md ---")
            .expect("the home file");
        assert!(data_at < file_at);
        assert!(
            !slot(1).contains("standing-orders"),
            "not in the soul's slot"
        );
        assert!(!slot(1).contains("Never send anything to Marta"));
        assert!(slot(6).contains(crate::bots::context_files::UNTRUSTED_PREAMBLE));
        assert_eq!(
            prompt.notes,
            [
                "_skills/My_Skill is not offered: Skill name 'My_Skill' must be lowercase Skill name \
                 'My_Skill' contains invalid characters. Only letters, digits, and hyphens are allowed.",
                "_skills/mismatch is not offered: Directory name 'mismatch' must match skill name 'matched'",
            ]
        );
    }

    #[test]
    fn the_digest_is_of_what_was_sent_and_told_is_the_same_text() {
        let home = nixi();
        let prompt = compose_home(&home);
        assert_eq!(
            prompt.prompt_sha256,
            hex::encode(Sha256::digest(prompt.text.as_bytes()))
        );
        assert_eq!(prompt.memory_sha256, home.memory.sha256);
        let told = told(&prompt);
        let joined: String = told.sections.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, prompt.text);
        assert_eq!(told.prompt_sha256, prompt.prompt_sha256);
        assert_eq!(told.notes, prompt.notes);
    }

    #[test]
    fn an_empty_menu_and_no_context_leave_their_slots_out() {
        let home = nixi();
        let prompt = compose(&PromptInput {
            soul: &home.soul,
            facts: &home.facts,
            memory: &home.memory,
            skills: &home.skills,
            menu: &[],
            frame: &home.frame,
            context: None,
        });
        let slots: Vec<u8> = prompt.sections.iter().map(|s| s.slot).collect();
        assert_eq!(slots, [1, 2, 3, 5]);
        assert!(prompt.text.find(FILE_CONTENT_IS_DATA) < prompt.text.find("--- home file:"));
        let joined: String = told(&prompt)
            .sections
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(joined, prompt.text);
    }

    #[test]
    fn a_memory_file_left_out_is_said_in_the_prompt_and_the_notes() {
        let mut home = nixi();
        let over = fixture("zone-ok/nixi/USER.md").replacen("tgorka is", "tgorka  is", 1);
        home.memory = memory::snapshot(Some(&over), Some(&fixture("zone-ok/nixi/MEMORY.md")));
        let prompt = compose_home(&home);
        let sentence = "USER.md is 1376 characters; the cap is 1375. Shorten it; keeper does not cut it for you.";
        assert!(prompt
            .text
            .contains(&format!("Left out of this session: {sentence}")));
        assert!(
            !prompt.text.contains("tgorka  is Tomasz"),
            "the over-cap file is not sent"
        );
        assert_eq!(prompt.notes[0], sentence);
    }
}
