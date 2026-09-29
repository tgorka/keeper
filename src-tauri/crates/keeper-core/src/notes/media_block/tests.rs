use super::*;
use crate::transcription::engine::TranscriptionLanguage;
use crate::transcription::model::{
    EngineStamp, MatchStatus, SourceKind, Speaker, TranscriptSource,
};
use crate::transcription::plan::TrackOrigin;

const ID: &str = "01K0DEVICE0000000000000000-01K0SESSION00000000000000";

fn refused(body: &str) -> BlockRefusal {
    parse(body).expect_err("the body is refused")
}

// --- Grammar -------------------------------------------------------------

#[test]
fn each_of_the_four_sources_reads() {
    assert_eq!(
        parse(&format!("session = \"{ID}\""))
            .expect("session")
            .source,
        Source::Session(ID.to_owned())
    );
    assert_eq!(
        parse("transcript = \"talks/a.mp4.transcript.json\"")
            .expect("transcript")
            .source,
        Source::Transcript("talks/a.mp4.transcript.json".to_owned())
    );
    assert_eq!(
        parse("src = \"meetings/kelly.toml\"").expect("src").source,
        Source::Src("meetings/kelly.toml".to_owned())
    );
    let parts = parse(
        "[[part]]\nfile = \"a/screen.mov\"\ncamera = \"a/cam.mov\"\nsystem = 1\nmicrophone = 2\n\n\
         [[part]]\nfile = \"a/b.m4a\"\noffset = \"00:10:00\"\n",
    )
    .expect("parts");
    let Source::Parts(parts) = parts.source else {
        panic!("parts");
    };
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].camera.as_deref(), Some("a/cam.mov"));
    assert_eq!(parts[1].offset, Some(600.0));
}

#[test]
fn two_sources_or_none_are_refused() {
    assert_eq!(
        refused(&format!("session = \"{ID}\"\nsrc = \"x.toml\"")),
        BlockRefusal::TwoSources {
            first: "session",
            second: "src"
        }
    );
    assert_eq!(refused("title = \"Nothing\""), BlockRefusal::NoSource);
    assert_eq!(refused(""), BlockRefusal::NoSource);
}

#[test]
fn an_unknown_key_is_refused_by_its_name() {
    let refusal = refused(&format!("session = \"{ID}\"\nform = \"00:12:00\""));
    assert_eq!(
        refusal,
        BlockRefusal::UnknownKey {
            key: "form".to_owned()
        }
    );
    assert!(refusal.to_string().contains("`form`"), "{refusal}");
    assert_eq!(
        refused("[[part]]\nfile = \"a.mov\"\nscreen = \"b.mov\""),
        BlockRefusal::UnknownTableKey {
            table: "part",
            key: "screen".to_owned()
        }
    );
}

#[test]
fn a_newer_version_is_refused_with_the_newer_keeper_sentence() {
    let refusal = refused(&format!("version = 2\nsession = \"{ID}\""));
    assert_eq!(refusal, BlockRefusal::NewerVersion);
    assert_eq!(
        refusal.to_string(),
        "This block was written by a newer keeper."
    );
    assert!(parse(&format!("version = 1\nsession = \"{ID}\"")).is_ok());
}

#[test]
fn a_src_file_that_names_src_is_refused() {
    assert_eq!(
        parse_src("src = \"other.toml\"").expect_err("nested"),
        BlockRefusal::NestedSrc
    );
    assert!(parse_src(&format!("session = \"{ID}\"")).is_ok());
}

#[test]
fn a_marker_needs_exactly_one_kind_of_time() {
    let body = |marker: &str| format!("session = \"{ID}\"\n\n[[marker]]\nname = \"M\"\n{marker}");
    assert_eq!(
        refused(&body("at = 5\nfrom = 1\nto = 9")),
        BlockRefusal::MarkerBoth {
            name: "M".to_owned()
        }
    );
    assert_eq!(
        refused(&body("")),
        BlockRefusal::MarkerTimeless {
            name: "M".to_owned()
        }
    );
    assert_eq!(
        refused(&body("from = 1")),
        BlockRefusal::MarkerTimeless {
            name: "M".to_owned()
        }
    );
}

#[test]
fn a_name_a_wikilink_cannot_carry_or_too_long_is_refused() {
    for name in ["a[b", "a]b", "a|b", "a#b", "a^b", "a\nb", "a\rb"] {
        assert_eq!(
            check_name(name),
            Err(BlockRefusal::NameForbidden),
            "{name:?}"
        );
    }
    assert_eq!(
        check_name(&"ą".repeat(80)).as_deref(),
        Ok("ą".repeat(80).as_str())
    );
    assert_eq!(check_name(&"ą".repeat(81)), Err(BlockRefusal::NameLength));
    assert_eq!(check_name("   "), Err(BlockRefusal::NameLength));
    assert_eq!(check_name("  Umowa ").as_deref(), Ok("Umowa"));
}

#[test]
fn names_are_unique_under_unicode_folding() {
    for (first, second) in [
        ("Umowa", "umowa"),
        ("umowa", "Umowa"),
        ("Ą", "ą"),
        ("ą", "Ą"),
    ] {
        let body = format!(
            "session = \"{ID}\"\n[[marker]]\nname = \"{first}\"\nat = 1\n[[marker]]\nname = \"{second}\"\nat = 2\n"
        );
        assert_eq!(
            refused(&body),
            BlockRefusal::DuplicateName {
                name: second.to_owned()
            }
        );
    }
}

// --- Times ---------------------------------------------------------------

#[test]
fn normal_play_time_reads_the_three_forms_and_numbers() {
    for (text, seconds) in [
        ("95", 95.0),
        ("95.5", 95.5),
        ("01:35", 95.0),
        ("1:02:30", 3_750.0),
        ("00:01:35.250", 95.25),
        ("npt:00:00:10", 10.0),
    ] {
        assert_eq!(parse_time(text), Some(seconds), "{text}");
    }
    for text in [
        "1:35", "10:60", "-5", "1:2:3", "", "1:00:60", "a", "01:5.5", "1::00",
    ] {
        assert_eq!(parse_time(text), None, "{text}");
    }
    let block = parse(&format!("session = \"{ID}\"\nfrom = 95\nto = 95.5")).expect("numbers");
    assert_eq!((block.from, block.to), (Some(95.0), Some(95.5)));
    assert_eq!(
        refused(&format!("session = \"{ID}\"\nfrom = -5")),
        BlockRefusal::BadTime {
            key: "from".to_owned(),
            text: "-5".to_owned()
        }
    );
}

#[test]
fn keeper_writes_hh_mm_ss() {
    assert_eq!(format_time(95.9), "00:01:35");
    assert_eq!(format_time(3_750.0), "01:02:30");
}

#[test]
fn an_empty_or_backwards_window_is_refused() {
    assert_eq!(
        refused(&format!(
            "session = \"{ID}\"\nfrom = \"00:10:00\"\nto = \"00:10:00\""
        )),
        BlockRefusal::EmptyWindow
    );
    assert_eq!(
        refused(&format!("session = \"{ID}\"\nfrom = 20\nto = 10")),
        BlockRefusal::EmptyWindow
    );
    assert!(parse(&format!("session = \"{ID}\"\nfrom = 10\nto = 20")).is_ok());
}

#[test]
fn a_parts_track_one_is_the_files_first_audio_track() {
    let block = parse("[[part]]\nfile = \"a.mov\"\nsystem = 1\nmicrophone = 2").expect("part");
    let Source::Parts(parts) = block.source else {
        panic!("parts");
    };
    assert_eq!(parts[0].system_index(), Some(0));
    assert_eq!(parts[0].microphone_index(), Some(1));
    assert_eq!(
        refused("[[part]]\nfile = \"a.mov\"\nsystem = 0"),
        BlockRefusal::TrackZero
    );
}

// --- Edits ---------------------------------------------------------------

/// A comment line, a blank line and an inline comment: what an edit must
/// leave byte for byte.
const EDITABLE: &str = "# Kelly, the pricing call.\nsession = \"01A-01B\"   # the id\n\ntitle = \"Pricing\"\n\n[[marker]]\nname = \"Price\"  # agreed\nat = \"00:13:05\"\n\n# the demo\n[[marker]]\nname = \"Demo\"\nfrom = \"00:14:10\"\nto = \"00:15:00\"\n";

#[test]
fn adding_a_marker_appends_one_table_and_changes_no_other_byte() {
    let moment = edit(
        EDITABLE,
        &MarkerEditReq::Add {
            name: " Next steps ".to_owned(),
            from: 785.8,
            to: None,
        },
    )
    .expect("added");
    assert_eq!(
        moment,
        format!("{EDITABLE}\n[[marker]]\nname = \"Next steps\"\nat = \"00:13:05\"\n")
    );
    assert_eq!(parse(&moment).expect("reads").markers.len(), 3);

    let window = edit(
        EDITABLE,
        &MarkerEditReq::Add {
            name: "Budget".to_owned(),
            from: 60.4,
            to: Some(90.2),
        },
    )
    .expect("added");
    assert_eq!(
        window,
        format!(
            "{EDITABLE}\n[[marker]]\nname = \"Budget\"\nfrom = \"00:01:00\"\nto = \"00:01:31\"\n"
        ),
        "from rounds down and to rounds up"
    );
}

#[test]
fn adding_a_duplicate_or_forbidden_name_writes_nothing() {
    assert_eq!(
        edit(
            EDITABLE,
            &MarkerEditReq::Add {
                name: "price".to_owned(),
                from: 1.0,
                to: None
            }
        ),
        Err(BlockRefusal::DuplicateName {
            name: "price".to_owned()
        })
    );
    assert_eq!(
        edit(
            EDITABLE,
            &MarkerEditReq::Add {
                name: "a#b".to_owned(),
                from: 1.0,
                to: None
            }
        ),
        Err(BlockRefusal::NameForbidden)
    );
}

#[test]
fn renaming_changes_only_that_name() {
    let renamed = edit(
        EDITABLE,
        &MarkerEditReq::Rename {
            name: "price".to_owned(),
            new_name: "Cena".to_owned(),
        },
    )
    .expect("renamed");
    assert_eq!(
        renamed,
        EDITABLE.replace("name = \"Price\"  # agreed", "name = \"Cena\"  # agreed")
    );
    assert_eq!(
        edit(
            EDITABLE,
            &MarkerEditReq::Rename {
                name: "Price".to_owned(),
                new_name: "demo".to_owned()
            }
        ),
        Err(BlockRefusal::DuplicateName {
            name: "demo".to_owned()
        })
    );
    assert_eq!(
        edit(
            EDITABLE,
            &MarkerEditReq::Rename {
                name: "Price".to_owned(),
                new_name: "PRICE".to_owned()
            }
        )
        .expect("a change of case is a rename"),
        EDITABLE.replace("\"Price\"", "\"PRICE\"")
    );
}

#[test]
fn removing_deletes_only_that_table() {
    let removed = edit(
        EDITABLE,
        &MarkerEditReq::Remove {
            name: "Price".to_owned(),
        },
    )
    .expect("removed");
    assert_eq!(
        removed,
        EDITABLE.replace(
            "[[marker]]\nname = \"Price\"  # agreed\nat = \"00:13:05\"\n\n",
            ""
        )
    );
    let both = edit(
        &removed,
        &MarkerEditReq::Remove {
            name: "demo".to_owned(),
        },
    )
    .expect("removed");
    assert_eq!(parse(&both).expect("reads").markers, Vec::new());
    assert!(both.starts_with(
        "# Kelly, the pricing call.\nsession = \"01A-01B\"   # the id\n\ntitle = \"Pricing\"\n"
    ));
    assert_eq!(
        edit(
            EDITABLE,
            &MarkerEditReq::Remove {
                name: "Nope".to_owned()
            }
        ),
        Err(BlockRefusal::NoSuchMarker {
            name: "Nope".to_owned()
        })
    );
}

#[test]
fn an_edit_of_a_body_that_does_not_read_is_refused() {
    let broken = "session = \"x\"\nform = 1\n";
    assert_eq!(
        edit(
            broken,
            &MarkerEditReq::Add {
                name: "A".to_owned(),
                from: 1.0,
                to: None
            }
        ),
        Err(BlockRefusal::UnknownKey {
            key: "form".to_owned()
        })
    );
    assert!(matches!(
        edit(
            "session = ",
            &MarkerEditReq::Remove {
                name: "A".to_owned()
            }
        ),
        Err(BlockRefusal::Syntax(_))
    ));
}

#[test]
fn an_inline_marker_array_gains_an_inline_table() {
    let body = "session = \"x\"\nmarker = [{ name = \"A\", at = 1 }]\n";
    let added = edit(
        body,
        &MarkerEditReq::Add {
            name: "B".to_owned(),
            from: 2.0,
            to: None,
        },
    )
    .expect("added");
    let names: Vec<String> = parse(&added)
        .expect("still reads")
        .markers
        .into_iter()
        .map(|marker| marker.name)
        .collect();
    assert_eq!(names, ["A", "B"]);
    assert!(added.starts_with("session = \"x\"\n"));
}

// --- Clips ---------------------------------------------------------------

fn utterance(id: &str, speaker: &str, start: f64, end: f64, text: &str) -> Utterance {
    Utterance {
        id: id.to_owned(),
        speaker: speaker.to_owned(),
        origin: TrackOrigin::System,
        start,
        end,
        text: text.to_owned(),
        asr_text: text.to_owned(),
        edited: false,
        words: Vec::new(),
    }
}

fn transcript() -> Transcript {
    Transcript {
        version: 1,
        source: TranscriptSource {
            kind: SourceKind::Recording,
            files: Vec::new(),
            parts: Vec::new(),
            title: Some("Kelly sync".to_owned()),
        },
        created_at: String::new(),
        engine: EngineStamp {
            asr: String::new(),
            diarizer: String::new(),
            embedding: String::new(),
        },
        language: TranscriptionLanguage::Auto,
        duration: 1_000.0,
        speakers: vec![Speaker {
            id: "S1".to_owned(),
            origin: TrackOrigin::System,
            person_id: None,
            name: Some("Kelly Chang".to_owned()),
            status: MatchStatus::Auto,
            score: None,
            candidates: Vec::new(),
            embedding: None,
            clip: None,
        }],
        utterances: vec![
            utterance("u1", "S1", 700.0, 720.5, "Before."),
            utterance(
                "u2",
                "S1",
                719.0,
                730.0,
                "So the price we can live with is…",
            ),
            utterance("u3", "ME", 730.0, 740.0, "For the first year, yes."),
            utterance("u4", "S1", 930.0, 935.0, "After."),
        ],
        dictionary_applied: Vec::new(),
        corrected: false,
    }
}

const CLIPPABLE: &str = "session = \"01A-01B\"  # a comment keeper drops\ntitle = \"Pricing\"\npicture = \"screen\"\n\n[[marker]]\nname = \"Inside\"\nat = \"00:12:30\"\n\n[[marker]]\nname = \"Straddles\"\nfrom = \"00:11:00\"\nto = \"00:12:10\"\n\n[[marker]]\nname = \"Window\"\nfrom = \"00:12:10\"\nto = \"00:15:00\"\n";

fn window(from: &str, to: &str) -> ClipWindow {
    ClipWindow::parse(Some(from), Some(to)).expect("window")
}

#[test]
fn a_clip_keeps_the_source_the_choices_and_the_markers_wholly_inside() {
    let clip = clip_block(CLIPPABLE, window("00:12:00", "00:15:30"), None, false).expect("clip");
    assert_eq!(
        clip.markdown,
        "```keeper-media\nsession = \"01A-01B\"\ntitle = \"Pricing\"\nfrom = \"00:12:00\"\nto = \"00:15:30\"\npicture = \"screen\"\n\n[[marker]]\nname = \"Inside\"\nat = \"00:12:30\"\n\n[[marker]]\nname = \"Window\"\nfrom = \"00:12:10\"\nto = \"00:15:00\"\n```\n"
    );
    assert_eq!(clip.lines, 0, "no transcript, no lines to count");
}

#[test]
fn a_clip_keeps_a_transcript_or_src_source_verbatim() {
    for source in [
        "transcript = \"talks/a.mp4.transcript.json\"",
        "src = \"meetings/kelly.toml\"",
    ] {
        let clip = clip_block(source, window("10", "20"), None, false).expect("clip");
        assert_eq!(
            clip.markdown,
            format!("```keeper-media\n{source}\nfrom = \"00:00:10\"\nto = \"00:00:20\"\n```\n")
        );
    }
    let parts = clip_block(
        "[[part]]\nfile = \"a.mov\"\nsystem = 1\n",
        window("10", "20"),
        None,
        false,
    )
    .expect("clip");
    assert!(parts
        .markdown
        .contains("\n[[part]]\nfile = \"a.mov\"\nsystem = 1\n"));
}

#[test]
fn a_clip_reaching_outside_the_blocks_own_window_is_refused() {
    let windowed = format!("session = \"{ID}\"\nfrom = \"00:12:00\"\nto = \"00:15:30\"");
    assert_eq!(
        clip_block(&windowed, window("00:11:59", "00:13:00"), None, false),
        Err(BlockRefusal::ClipOutsideWindow)
    );
    assert_eq!(
        clip_block(&windowed, window("00:12:00", "00:15:31"), None, false),
        Err(BlockRefusal::ClipOutsideWindow)
    );
    assert!(clip_block(&windowed, window("00:12:00", "00:15:30"), None, false).is_ok());
    assert!(
        clip_block(&windowed, ClipWindow::default(), None, false).is_ok(),
        "no window asked is the block's own"
    );
}

#[test]
fn a_clips_words_are_the_lines_overlapping_the_window_in_the_twins_format() {
    let t = transcript();
    let clip = clip_block(CLIPPABLE, window("00:12:00", "00:15:30"), Some(&t), true).expect("clip");
    assert_eq!(clip.lines, 3);
    let words = clip
        .markdown
        .split("```\n")
        .nth(1)
        .expect("the callout after the fence");
    assert_eq!(
        words,
        "> [!transcript]- Pricing · 00:12:00–00:15:30\n\
         > **[00:11:40] Kelly Chang:** Before.\n\
         > **[00:11:59] Kelly Chang:** So the price we can live with is…\n\
         > **[00:12:10] ME:** For the first year, yes.\n"
    );
    let without =
        clip_block(CLIPPABLE, window("00:12:00", "00:15:30"), Some(&t), false).expect("clip");
    assert!(without.markdown.ends_with("```\n"));
    assert_eq!(without.lines, 3);
}

#[test]
fn a_clip_from_the_viewer_names_the_session_or_the_transcript() {
    let t = transcript();
    let session = clip_transcript(
        &ClipSource::Session(ID.to_owned()),
        ClipWindow::default(),
        &t,
        false,
    );
    assert_eq!(session.markdown, session_block(ID));
    assert_eq!(session.lines, 4);
    let path = clip_transcript(
        &ClipSource::Transcript("talks/a.mp4.transcript.json".to_owned()),
        window("00:12:00", "00:12:15"),
        &t,
        false,
    );
    assert_eq!(
        path.markdown,
        "```keeper-media\ntranscript = \"talks/a.mp4.transcript.json\"\nfrom = \"00:12:00\"\nto = \"00:12:15\"\n```\n"
    );
    assert_eq!(path.lines, 3, "a line that ends inside the window counts");
}

#[test]
fn a_clip_window_is_checked_as_typed() {
    assert_eq!(
        ClipWindow::parse(Some("1:35"), None),
        Err(BlockRefusal::BadTime {
            key: "from".to_owned(),
            text: "1:35".to_owned()
        })
    );
    assert_eq!(
        ClipWindow::parse(Some("00:10:00"), Some("00:09:00")),
        Err(BlockRefusal::EmptyWindow)
    );
    assert_eq!(
        ClipWindow::parse(Some(" "), None),
        Ok(ClipWindow::default())
    );
}

// --- Blocks in a note ----------------------------------------------------

#[test]
fn the_stubs_block_is_three_lines_naming_the_session() {
    let block = session_block(ID);
    assert_eq!(block, format!("```keeper-media\nsession = \"{ID}\"\n```\n"));
    let found = blocks(&block);
    assert_eq!(
        parse(&found[0].body).expect("reads").source,
        Source::Session(ID.to_owned())
    );
}

#[test]
fn a_block_is_found_with_backticks_tildes_indented_or_in_a_list() {
    let note = "# N\n\n~~~keeper-media\nsession = \"a\"\n~~~\n\n- item\n\n    ```keeper-media title\n    session = \"b\"\n    ```\n\n````markdown\n```keeper-media\nsession = \"quoted\"\n```\n````\n\n```toml\nsession = \"c\"\n```\n```mermaid\ngraph\n```\n";
    let found = blocks(note);
    let bodies: Vec<&str> = found.iter().map(|block| block.body.as_str()).collect();
    assert_eq!(bodies, ["session = \"a\"\n", "session = \"b\"\n"]);
    assert_eq!((found[0].first_line, found[0].last_line), (3, 5));
    assert_eq!(session_ids(note), ["a", "b"]);
}

#[test]
fn a_marker_is_found_in_the_first_block_holding_it() {
    let first = parse(&format!("session = \"{ID}\"")).expect("a");
    let second = parse(&format!(
        "session = \"{ID}\"\n[[marker]]\nname = \"The price we agreed\"\nat = \"00:13:05\""
    ))
    .expect("b");
    let hit =
        find_marker([None, Some(&first), Some(&second)], "the price WE agreed").expect("found");
    assert_eq!(
        hit,
        MediaMarkerHitVm {
            block: 2,
            name: "The price we agreed".to_owned(),
            from: 785.0,
            to: None
        }
    );
    assert_eq!(find_marker([Some(&first)], "nope"), None);
}

// --- Play in a player ----------------------------------------------------

#[test]
fn a_recordings_embeds_collapse_into_one_block_at_the_first() {
    let note = "---\nsession: x\n---\n# T\n\n![[r/screen-0000.mov]]\n![[r/camera-0000.mov]]\n\nwrite here ![[r/screen-0001.mov]] more\n![[elsewhere.png]]\n";
    let edits = session_embeds_to_block(note, ID, 7, |target| target.starts_with("r/"));
    assert_eq!(
        edits,
        [
            LineEditVm {
                first_line: 6,
                last_line: 6,
                text: Some(session_block(ID).trim_end().to_owned())
            },
            LineEditVm {
                first_line: 7,
                last_line: 7,
                text: None
            },
            LineEditVm {
                first_line: 9,
                last_line: 9,
                text: Some("write here  more".to_owned())
            },
        ]
    );
    assert!(
        session_embeds_to_block(note, ID, 10, |target| target.starts_with("r/")).is_empty(),
        "the embed asked about is not the recording's"
    );
}

#[test]
fn a_plain_media_embed_becomes_a_one_part_block_on_its_own_lines() {
    let note = "# T\nsee ![[clip.mp4]] here\n![[clip.mp4]]\n";
    assert_eq!(
        file_embed_to_block(note, 3, "clip.mp4", "notes/clip.mp4"),
        Some(LineEditVm {
            first_line: 3,
            last_line: 3,
            text: Some("```keeper-media\n[[part]]\nfile = \"notes/clip.mp4\"\n```".to_owned())
        })
    );
    assert_eq!(
        file_embed_to_block(note, 2, "clip.mp4", "notes/clip.mp4").and_then(|edit| edit.text),
        Some("see  here\n```keeper-media\n[[part]]\nfile = \"notes/clip.mp4\"\n```".to_owned())
    );
    assert_eq!(file_embed_to_block(note, 1, "clip.mp4", "x"), None);
}

// --- Adopting old stubs --------------------------------------------------

const OLD_STUB: &str = "---\ntitle: Kelly sync\nsession: 01A-01B\nrecording: 2026/kelly\nfiles:\n  - 2026/kelly/screen-0000.mov\n  - 2026/kelly/camera-0000.mov\n  - 2026/kelly/manifest.json\n---\n\n# Kelly sync\n\n![[2026/kelly/screen-0000.mov]]\n![[2026/kelly/camera-0000.mov]]\n\nWhat we agreed.\n";

#[test]
fn an_old_stub_becomes_one_block_and_every_other_byte_stays() {
    let Adoption::Changed(adopted) = adopt(OLD_STUB) else {
        panic!("an untouched stub is adopted");
    };
    assert_eq!(
        adopted,
        OLD_STUB.replace(
            "![[2026/kelly/screen-0000.mov]]\n![[2026/kelly/camera-0000.mov]]\n",
            "```keeper-media\nsession = \"01A-01B\"\n```\n"
        )
    );
    assert_eq!(adopt(&adopted), Adoption::Untouched, "idempotent");
}

#[test]
fn a_stub_whose_embeds_were_edited_by_hand_is_skipped() {
    for edited in [
        OLD_STUB.replace("![[2026/kelly/camera-0000.mov]]\n", ""),
        OLD_STUB.replace(
            "![[2026/kelly/camera-0000.mov]]\n",
            "Kelly's face:\n![[2026/kelly/camera-0000.mov]]\n",
        ),
        OLD_STUB.replace("What we agreed.", "Again: ![[2026/kelly/screen-0000.mov]]"),
    ] {
        assert_eq!(adopt(&edited), Adoption::Skipped, "{edited}");
    }
}

#[test]
fn a_note_that_is_not_a_recording_stub_is_untouched() {
    assert_eq!(adopt("# Plain\n\n![[clip.mov]]\n"), Adoption::Untouched);
    let no_embeds = OLD_STUB.replace(
        "![[2026/kelly/screen-0000.mov]]\n![[2026/kelly/camera-0000.mov]]\n",
        "",
    );
    assert_eq!(adopt(&no_embeds), Adoption::Untouched);
}
