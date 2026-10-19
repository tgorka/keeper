//! What the live turn tests share: the person's client, an answer as the
//! person saw it, the scripted models that read the attachments, and every
//! kind of file sent and answered (story 102.4 #5) — run against agentd and
//! against the desktop host's own entry alike.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keeper_core::agents::events;
use keeper_core::agents::matrix::AgentClient;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};
use serde_json::{json, Value};

use super::{record, syncing, Smoke};

/// The person's client, joined to the session room and syncing.
pub async fn person(
    smoke: &Smoke,
    dir: &Path,
    room: &OwnedRoomId,
) -> (
    AgentClient,
    Arc<Mutex<Vec<(Instant, Value)>>>,
    tokio::task::JoinHandle<()>,
) {
    let me = smoke.user("tgorka-smoke");
    smoke.clear_devices(&me).await;
    let client = smoke
        .client(dir, "tgorka-smoke", smoke.secret("TGORKA_SMOKE_PASSWORD"))
        .await;
    client.join(room).await.expect("join");
    client.sync_once().await.expect("sync");
    let seen = record(&client);
    let sync = syncing(&client);
    (client, seen, sync)
}

/// One answered turn as the person saw it.
#[derive(Debug)]
pub struct Seen {
    /// Every edit's `origin_server_ts`, the anchor's first, the final last.
    pub times: Vec<u64>,
    pub final_text: String,
}

/// Wait for the agent's final edit, answering the event `sent`, whose text
/// `done` accepts.
pub async fn answer_to(
    seen: &Arc<Mutex<Vec<(Instant, Value)>>>,
    agent: &OwnedUserId,
    sent: &str,
    question: &str,
    done: &dyn Fn(&str) -> bool,
) -> Seen {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let events: Vec<Value> = seen
            .lock()
            .expect("lock")
            .iter()
            .map(|(_, v)| v.clone())
            .collect();
        let anchor = events.iter().find(|e| {
            e["sender"] == agent.as_str()
                && e["content"][events::TURN].is_object()
                && events.iter().any(|q| q["event_id"] == sent)
                && e["origin_server_ts"].as_u64()
                    >= events
                        .iter()
                        .find(|q| q["event_id"] == sent)
                        .and_then(|q| q["origin_server_ts"].as_u64())
        });
        if let Some(anchor) = anchor {
            let anchor_id = anchor["event_id"].as_str().expect("id").to_owned();
            let edits: Vec<&Value> = events
                .iter()
                .filter(|e| e["content"]["m.relates_to"]["event_id"] == anchor_id.as_str())
                .collect();
            if let Some(last) = edits.last() {
                let text = last["content"]["m.new_content"]["body"]
                    .as_str()
                    .unwrap_or_default();
                if done(text) {
                    let mut times = vec![anchor["origin_server_ts"].as_u64().expect("ts")];
                    times.extend(edits.iter().filter_map(|e| e["origin_server_ts"].as_u64()));
                    return Seen {
                        times,
                        final_text: text.to_owned(),
                    };
                }
            }
        }
        assert!(Instant::now() < deadline, "no answer to {question:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A message's text: its string content, or its text parts joined.
pub fn message_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// The model of the attachments test: it reads, with `drive_read`, every
/// `.txt` and `.csv` its newest user message lists under "Attached files",
/// one call a round, then answers with what the reads returned.
pub fn reads_attachments(messages: &[Value]) -> Value {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let start = messages
        .iter()
        .rposition(|m| m["role"] == "user")
        .unwrap_or(0);
    let said = message_text(&messages[start]);
    let files: Vec<String> = said
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- smoke:"))
        .filter(|path| path.ends_with(".txt") || path.ends_with(".csv"))
        .map(str::to_owned)
        .collect();
    let read: Vec<String> = messages[start + 1..]
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(message_text)
        .collect();
    if let Some(next) = files.get(read.len()) {
        let id = format!(
            "call_{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let args = json!({ "path": next }).to_string();
        return json!({"model":"stub","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":"drive_read","arguments":args}}]},"finish_reason":"tool_calls"}]});
    }
    let answer = format!("From the files: {}", read.join(" | "));
    json!({"model":"stub","choices":[{"index":0,"delta":{"content":answer},"finish_reason":"stop"}]})
}

/// The model of the every-kind test: [`reads_attachments`]'s reads, then an
/// answer naming what it read and every picture its newest user message
/// showed it, as `mime:base64 length`.
pub fn tells_what_it_got(messages: &[Value]) -> Value {
    let frame = reads_attachments(messages);
    let Some(read) = frame["choices"][0]["delta"]["content"].as_str() else {
        return frame;
    };
    let start = messages
        .iter()
        .rposition(|m| m["role"] == "user")
        .unwrap_or(0);
    let shown: Vec<String> = messages[start]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["image_url"]["url"].as_str())
        .filter_map(|url| {
            let (mime, base64) = url.strip_prefix("data:")?.split_once(";base64,")?;
            Some(format!("{mime}:{}", base64.len()))
        })
        .collect();
    let answer = format!("{read} | shown: [{}]", shown.join(", "));
    json!({"model":"stub","choices":[{"index":0,"delta":{"content":answer},"finish_reason":"stop"}]})
}

/// `side` pixels square of noise no codec compresses, as a PNG.
pub fn noise_png(side: u32) -> Vec<u8> {
    use image::ImageEncoder;
    let mut state = 0x2545_f491_u32;
    let pixels: Vec<u8> = (0..side * side * 3)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 24) as u8
        })
        .collect();
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&pixels, side, side, image::ExtendedColorType::Rgb8)
        .expect("png");
    png
}

/// Story 102.4 #5: the eight kinds of file sent into `room` as `client` —
/// each answered by `agent`, what it was said or read in the answer, the
/// large photo shown as a JPEG within Anthropic's limit under a `running`
/// status said before its anchor. `log` is the host's log, for a failure.
pub async fn every_kind_is_answered(
    client: &AgentClient,
    seen: &Arc<Mutex<Vec<(Instant, Value)>>>,
    agent: &OwnedUserId,
    room: &OwnedRoomId,
    log: &dyn Fn() -> String,
) {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../keeper-agent/tests/fixtures/attachments");
    let fixture = |name: &str| std::fs::read(fixtures.join(name)).expect("fixture");
    let large = noise_png(1200);
    let limit = keeper_core::bots::deliverable::ANTHROPIC_MAX_IMAGE_BYTES;
    assert!(large.len() > limit, "{}", large.len());
    let noise: Vec<u8> = (0..4096_u32).map(|n| (n * 7 % 251) as u8).collect();
    let kinds = [
        (
            "m.image",
            "screen.png",
            "image/png",
            "What is on this screenshot?",
            noise_png(8),
            "shown: [image/png:",
        ),
        (
            "m.image",
            "large.png",
            "image/png",
            "",
            large,
            "shown: [image/jpeg:",
        ),
        (
            "m.file",
            "quarterly.pdf",
            "application/pdf",
            "What is the total?",
            fixture("quarterly.pdf"),
            "Quarterly total 4217",
        ),
        (
            "m.file",
            "devices.csv",
            "text/csv",
            "",
            fixture("devices.csv"),
            "hesperia,tgorka",
        ),
        (
            "m.file",
            "plan.txt",
            "text/plain",
            "Is the plan ready?",
            b"The plan is ready.".to_vec(),
            "The plan is ready.",
        ),
        (
            "m.audio",
            "memo.m4a",
            "audio/mp4",
            "What do I say here?",
            noise.clone(),
            "keeper does not let agents listen to or watch memo.m4a yet; it was saved at ",
        ),
        (
            "m.video",
            "clip.mp4",
            "video/mp4",
            "",
            noise.clone(),
            "keeper does not let agents listen to or watch clip.mp4 yet; it was saved at ",
        ),
        (
            "m.image",
            "photo.heic",
            "image/heic",
            "",
            noise,
            "this photo's format (HEIC) can't be shown to the model",
        ),
    ];
    for (msgtype, name, mime, caption, bytes, said) in kinds {
        let file = client.upload_encrypted(&bytes).await.expect("upload");
        let sent = client
            .send(
                room,
                "m.room.message",
                json!({
                    "msgtype": msgtype,
                    "body": if caption.is_empty() { name } else { caption },
                    "filename": name,
                    "info": {"mimetype": mime, "size": bytes.len()},
                    "file": file,
                }),
                None,
            )
            .await
            .expect("send")
            .to_string();
        let answer = answer_to(seen, agent, &sent, name, &|text| {
            text.starts_with("From the files:")
        })
        .await;
        assert!(
            answer.final_text.contains(said),
            "{name}: {}\n{}",
            answer.final_text,
            log()
        );
        if name == "large.png" {
            let shown: usize = answer
                .final_text
                .split("image/jpeg:")
                .nth(1)
                .and_then(|rest| rest.split(']').next())
                .and_then(|length| length.parse().ok())
                .expect("the copy's length");
            assert!(shown <= 5 * 1024 * 1024, "base64 of {shown} bytes");
            // 102.4 #3: the status said `running` while the photo was taken
            // in, before the turn's anchor — never `idle` through it.
            let timeline: Vec<Value> = seen
                .lock()
                .expect("lock")
                .iter()
                .map(|(_, v)| v.clone())
                .collect();
            let asked_at = timeline
                .iter()
                .find(|e| e["event_id"] == sent.as_str())
                .and_then(|e| e["origin_server_ts"].as_u64())
                .expect("the photo's time");
            let running = timeline.iter().any(|e| {
                let content = if e["content"]["m.new_content"].is_object() {
                    &e["content"]["m.new_content"]
                } else {
                    &e["content"]
                };
                e["sender"] == agent.as_str()
                    && e["type"] == events::STATUS
                    && content["run"] == "running"
                    && e["origin_server_ts"]
                        .as_u64()
                        .is_some_and(|at| at >= asked_at && at <= answer.times[0])
            });
            assert!(running, "no running status before the anchor");
        }
    }
}
