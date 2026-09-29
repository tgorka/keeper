//! The markdown twin of a transcript (AD-344): what a person reads in any
//! editor, re-rendered from the JSON on every save and never parsed back.

use std::fmt::Write as _;

use super::model::{MatchStatus, Transcript};

/// `hh:mm:ss` for a time in seconds.
fn clock(seconds: f64) -> String {
    // Whole seconds of a finite, non-negative time; saturates on nonsense.
    let total = seconds.max(0.0).floor() as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        total / 60 % 60,
        total % 60
    )
}

fn status_word(status: MatchStatus) -> &'static str {
    match status {
        MatchStatus::Auto => "recognized",
        MatchStatus::Suggested => "maybe",
        MatchStatus::Confirmed => "confirmed",
        MatchStatus::Unknown => "not recognized",
        MatchStatus::Me => "microphone",
    }
}

/// The transcript as markdown: title, date, duration, a speaker legend (the
/// speakers with at least one line — one left lineless by a move is kept in
/// the JSON only so the move can be undone), one `**[hh:mm:ss] Name:** text`
/// paragraph per utterance, and a footer naming the models.
pub fn markdown(t: &Transcript) -> String {
    let mut out = String::new();
    let title = t
        .source
        .title
        .as_deref()
        .or_else(|| t.source.files.first().map(String::as_str))
        .unwrap_or("Transcript");
    let _ = writeln!(out, "# Transcript: {title}\n");
    let _ = writeln!(out, "- Date: {}", t.created_at);
    let _ = writeln!(out, "- Duration: {}", clock(t.duration));
    let _ = writeln!(out, "- Language: {}\n", t.language.as_wire());

    out.push_str("## Speakers\n\n");
    for speaker in t
        .speakers
        .iter()
        .filter(|speaker| t.utterances.iter().any(|u| u.speaker == speaker.id))
    {
        let _ = write!(
            out,
            "- **{}** ({}, {}",
            Transcript::display_name(speaker),
            speaker.id,
            status_word(speaker.status)
        );
        if let Some(score) = speaker.score {
            let _ = write!(out, " {score:.2}");
        }
        out.push_str(")\n");
    }

    out.push_str("\n## Transcript\n\n");
    for utterance in &t.utterances {
        let name = t
            .speaker(&utterance.speaker)
            .map_or_else(|| utterance.speaker.clone(), Transcript::display_name);
        let _ = writeln!(
            out,
            "**[{}] {name}:** {}\n",
            clock(utterance.start),
            utterance.text
        );
    }

    let _ = writeln!(
        out,
        "---\n\nTranscribed on this Mac with {}, {} and {}.",
        t.engine.asr, t.engine.diarizer, t.engine.embedding
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::assemble::tests::{asr, ctx};
    use crate::transcription::assemble::{assemble, PartResult};
    use crate::transcription::plan::TrackOrigin;

    #[test]
    fn the_markdown_names_each_speaker_and_stamps_each_line() {
        let mut context = ctx(&[]);
        context.source.files = vec!["screen-0000.mov".to_owned()];
        let mut t = assemble(
            &[
                PartResult {
                    offset: 3600.0,
                    origin: TrackOrigin::System,
                    asr: asr("hello from far away", 61.0, 0.5),
                    diar: None,
                },
                PartResult {
                    offset: 3600.0,
                    origin: TrackOrigin::Microphone,
                    asr: asr("and hello from here", 70.0, 0.5),
                    diar: None,
                },
            ],
            None,
            &[],
            context,
        );
        t.speakers[1].name = Some("Ada".to_owned());
        let md = markdown(&t);
        let lines: Vec<&str> = md.lines().collect();
        assert_eq!(lines[0], "# Transcript: screen-0000.mov");
        assert!(lines.contains(&"- Duration: 01:01:11"), "{md}");
        assert!(lines.contains(&"- **You** (ME, microphone)"), "{md}");
        assert!(lines.contains(&"- **Ada** (S1, not recognized)"), "{md}");
        assert!(
            lines.contains(&"**[01:01:01] Ada:** hello from far away"),
            "{md}"
        );
        assert!(
            lines.contains(&"**[01:01:10] You:** and hello from here"),
            "{md}"
        );
        assert!(lines
            .last()
            .is_some_and(|line| line.starts_with("Transcribed on this Mac with parakeet")));
        let mut moved = t.clone();
        for utterance in moved.utterances.iter_mut().filter(|u| u.speaker == "S1") {
            "ME".clone_into(&mut utterance.speaker);
        }
        let md = markdown(&moved);
        assert!(
            !md.contains("(S1,"),
            "a speaker without a line is not in the legend: {md}"
        );
        assert!(md.contains("- **You** (ME, microphone)"), "{md}");
        t.source.title = Some("2026-09-23 17.29 kelly-sync".to_owned());
        assert_eq!(
            markdown(&t).lines().next(),
            Some("# Transcript: 2026-09-23 17.29 kelly-sync"),
            "a session is headed by its folder, not its first segment"
        );
    }
}
