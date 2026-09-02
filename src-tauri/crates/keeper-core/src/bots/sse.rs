//! The server-sent-events framer, and nothing else.
//!
//! # Why this is its own file with no network in it
//!
//! `reqwest`'s body chunks correspond to TCP/HTTP2 frame arrival, **not** to
//! SSE frames (research §5.2): one chunk may hold three frames and half of a
//! fourth, and the next may complete it forty milliseconds later. Every
//! interesting failure of an SSE client is therefore a failure of buffering,
//! and buffering is testable without a socket — so it lives here, behind
//! [`SseFramer::push`], and the wire tests feed it hand-cut chunks directly.
//!
//! # What this framer implements
//!
//! The subset of the WHATWG event-stream grammar that an OpenAI-compatible
//! chat endpoint actually emits:
//!
//! * frames separated by a blank line, in any of the three line endings the
//!   grammar allows (`\n\n`, `\r\n\r\n`, `\r\r`);
//! * `data:` fields, with the single space after the colon optional and
//!   several `data:` lines in one frame joined by `\n`;
//! * `:`-prefixed comment lines, which are how both back ends keep the socket
//!   warm and which must not be mistaken for content;
//! * other fields (`event:`, `id:`, `retry:`) parsed and ignored — Hermes
//!   emits a named `hermes.tool.progress` event on this same stream (research
//!   §2.6) and a framer that tripped over it would break on the real server;
//! * the `[DONE]` sentinel, surfaced as its own frame kind;
//! * a `data:`-only frame with an empty payload, which dispatches nothing.
//!
//! # Two decisions worth stating
//!
//! **UTF-8 is decoded per frame, not per chunk.** A multi-byte scalar split
//! across two chunks is the classic bug here, and the classic fix is a
//! byte-level UTF-8 buffer that re-emits the tail. This framer does not need
//! one: it buffers *bytes* and only decodes once a frame is complete, and a
//! frame boundary is by definition a run of ASCII newlines, so no scalar can
//! straddle it. Invalid UTF-8 inside a complete frame is then a real error
//! rather than an artefact of chunking.
//!
//! **Buffering is bounded.** A hostile or broken endpoint can open a stream and
//! never send a blank line; the read timeout does not catch that, because the
//! bytes keep coming. [`MAX_FRAME_BYTES`] caps one frame's worth of
//! unterminated buffer and turns the overrun into an error.

use crate::bots::error::BotsError;

/// The largest single SSE frame keeper will assemble.
///
/// Generous against the real traffic — a `chat.completion.chunk` is a few
/// hundred bytes, and the largest legitimate frame is the final usage chunk —
/// and small enough that a stream which never sends a blank line fails in
/// under a megabyte of RAM instead of in whatever the machine has.
pub const MAX_FRAME_BYTES: usize = 1 << 20;

/// One dispatched event-stream frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseFrame {
    /// A frame carrying data, already joined across its `data:` lines.
    Data(String),
    /// The literal `data: [DONE]` sentinel that ends an OpenAI-compatible
    /// stream. Both back ends send it (research §2.6, §5.4.1).
    Done,
}

/// Assembles [`SseFrame`]s from arbitrarily-cut byte chunks.
///
/// Feed it with [`push`](Self::push) and drain it with
/// [`next_frame`](Self::next_frame) until that returns `None`.
#[derive(Debug)]
pub struct SseFramer {
    buffer: Vec<u8>,
    max_frame_bytes: usize,
}

impl Default for SseFramer {
    fn default() -> Self {
        Self::new()
    }
}

impl SseFramer {
    /// A framer with the default [`MAX_FRAME_BYTES`] cap.
    pub fn new() -> Self {
        Self::with_max_frame_bytes(MAX_FRAME_BYTES)
    }

    /// A framer with an explicit cap — the seam a test uses to prove the
    /// overrun without allocating a megabyte.
    pub fn with_max_frame_bytes(max_frame_bytes: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_frame_bytes,
        }
    }

    /// Append a chunk of body bytes. Chunk boundaries are arbitrary.
    pub fn push(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    /// Pop the next complete frame, if the buffer holds one.
    ///
    /// `Ok(None)` means "not yet, feed me more". It is also where the frame
    /// cap is enforced, because that is the moment it is known that the
    /// buffered bytes are one unterminated frame rather than several complete
    /// ones.
    pub fn next_frame(&mut self) -> Result<Option<SseFrame>, BotsError> {
        loop {
            let Some((end, separator_len)) = find_separator(&self.buffer) else {
                if self.buffer.len() > self.max_frame_bytes {
                    return Err(BotsError::Protocol {
                        detail: format!(
                            "a single event-stream frame exceeded {} bytes without ending",
                            self.max_frame_bytes
                        ),
                    });
                }
                return Ok(None);
            };
            let frame: Vec<u8> = self.buffer.drain(..end + separator_len).collect();
            let frame = &frame[..end];
            if let Some(dispatched) = parse_frame(frame)? {
                return Ok(Some(dispatched));
            }
            // A comment-only or empty frame dispatches nothing; keep looking so
            // a caller draining to `None` never mistakes a keep-alive for the
            // end of the buffer.
        }
    }

    /// How many bytes of an incomplete trailing frame are still buffered.
    ///
    /// At EOF the grammar discards an unterminated frame, and so does this
    /// framer — but a stream cut mid-frame is evidence, so the driver reads
    /// this to say *how* the stream ended rather than only that it did.
    pub fn pending_bytes(&self) -> usize {
        self.buffer.len()
    }
}

/// Find the first blank line, returning its offset and length.
///
/// All three endings the grammar allows are accepted. They cannot be confused
/// with one another: `\r\n\r\n` contains neither `\n\n` nor `\r\r` as a
/// substring, so the earliest match is unambiguous. A buffer ending mid
/// separator (`"…\r"` with the `"\n\r\n"` still in flight) simply does not
/// match, which is the correct "not yet".
fn find_separator(buffer: &[u8]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (needle, len) in [
        (b"\r\n\r\n".as_slice(), 4usize),
        (b"\n\n".as_slice(), 2),
        (b"\r\r".as_slice(), 2),
    ] {
        if let Some(at) = find(buffer, needle) {
            if best.is_none_or(|(current, _)| at < current) {
                best = Some((at, len));
            }
        }
    }
    best
}

/// First occurrence of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Turn one frame's bytes into a dispatched frame, or `None` when the frame
/// dispatches nothing (a keep-alive comment, or `data:` with no payload).
fn parse_frame(frame: &[u8]) -> Result<Option<SseFrame>, BotsError> {
    let text = std::str::from_utf8(frame).map_err(|err| BotsError::Parse {
        endpoint: "event-stream".to_owned(),
        detail: format!("a frame was not valid UTF-8 at byte {}", err.valid_up_to()),
    })?;

    let mut data = String::new();
    let mut saw_data_field = false;
    for line in text.split(['\n', '\r']).filter(|line| !line.is_empty()) {
        // A line beginning with a colon is a comment. Both back ends use these
        // as keep-alives, so this branch is the hot path on an idle stream.
        if line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            // "The single space after the colon is optional" — stripping
            // `"data: "` only, as one widely-copied client does, silently
            // mangles a server that omits it (research §5.2).
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            // A line with no colon is a field with an empty value.
            None => (line, ""),
        };
        if field != "data" {
            // `event:`, `id:`, `retry:` and anything a gateway invents. Ignored
            // by design: Hermes multiplexes `hermes.tool.progress` onto this
            // stream, and a client that errored on an unknown field would break
            // against the real server.
            continue;
        }
        saw_data_field = true;
        if !data.is_empty() {
            data.push('\n');
        }
        data.push_str(value);
    }

    if !saw_data_field || data.is_empty() {
        return Ok(None);
    }
    if data == "[DONE]" {
        return Ok(Some(SseFrame::Done));
    }
    Ok(Some(SseFrame::Data(data)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drain a framer completely, for the tests that only care about the
    /// sequence of frames a set of chunks produces.
    fn drain(chunks: &[&[u8]]) -> Vec<SseFrame> {
        let mut framer = SseFramer::new();
        let mut out = Vec::new();
        for chunk in chunks {
            framer.push(chunk);
            while let Some(frame) = framer.next_frame().expect("well-formed frames") {
                out.push(frame);
            }
        }
        out
    }

    #[test]
    fn a_frame_split_across_two_chunks_is_reassembled() {
        let frames = drain(&[b"data: {\"a\":", b"1}\n\ndata: [DONE]\n\n"]);
        assert_eq!(
            frames,
            vec![SseFrame::Data("{\"a\":1}".to_owned()), SseFrame::Done]
        );
    }

    #[test]
    fn a_separator_split_across_two_chunks_is_reassembled() {
        // The nastier cut: the blank line itself straddles the boundary.
        let frames = drain(&[b"data: one\r", b"\n\r\ndata: two\r\n\r\n"]);
        assert_eq!(
            frames,
            vec![
                SseFrame::Data("one".to_owned()),
                SseFrame::Data("two".to_owned())
            ]
        );
    }

    #[test]
    fn a_multi_byte_scalar_split_across_chunks_survives() {
        // The thumbs-up emoji, cut in half mid-scalar.
        let frames = drain(&[b"data: \xf0\x9f", b"\x91\x8d\n\n"]);
        assert_eq!(frames, vec![SseFrame::Data("\u{1f44d}".to_owned())]);
    }

    #[test]
    fn comment_keepalives_dispatch_nothing() {
        let frames = drain(&[b": keep-alive\n\n:\n\ndata: x\n\n"]);
        assert_eq!(frames, vec![SseFrame::Data("x".to_owned())]);
    }

    #[test]
    fn multi_line_data_is_joined_with_newlines() {
        let frames = drain(&[b"data: one\ndata: two\n\n"]);
        assert_eq!(frames, vec![SseFrame::Data("one\ntwo".to_owned())]);
    }

    #[test]
    fn a_data_line_without_the_optional_space_is_read() {
        let frames = drain(&[b"data:{\"a\":1}\n\n"]);
        assert_eq!(frames, vec![SseFrame::Data("{\"a\":1}".to_owned())]);
    }

    #[test]
    fn a_named_event_field_is_ignored_and_its_data_still_arrives() {
        // Hermes' tool-progress frames look exactly like this.
        let frames = drain(&[b"event: hermes.tool.progress\ndata: {\"tool\":\"x\"}\n\n"]);
        assert_eq!(frames, vec![SseFrame::Data("{\"tool\":\"x\"}".to_owned())]);
    }

    #[test]
    fn an_empty_data_field_dispatches_nothing() {
        let frames = drain(&[b"data:\n\ndata: x\n\n"]);
        assert_eq!(frames, vec![SseFrame::Data("x".to_owned())]);
    }

    #[test]
    fn a_truncated_final_frame_is_left_pending_and_counted() {
        let mut framer = SseFramer::new();
        framer.push(b"data: {\"a\":1}\n\ndata: {\"b\"");
        assert_eq!(
            framer.next_frame().expect("first frame"),
            Some(SseFrame::Data("{\"a\":1}".to_owned()))
        );
        assert_eq!(framer.next_frame().expect("no second frame"), None);
        assert_eq!(framer.pending_bytes(), b"data: {\"b\"".len());
    }

    #[test]
    fn a_frame_that_never_ends_is_capped_rather_than_buffered() {
        let mut framer = SseFramer::with_max_frame_bytes(64);
        framer.push(&[b'x'; 65]);
        let err = framer
            .next_frame()
            .expect_err("an unterminated frame past the cap must error");
        assert!(matches!(err, BotsError::Protocol { .. }));
    }

    #[test]
    fn invalid_utf8_inside_a_complete_frame_is_an_error() {
        let mut framer = SseFramer::new();
        framer.push(b"data: \xff\xfe\n\n");
        assert!(matches!(framer.next_frame(), Err(BotsError::Parse { .. })));
    }
}
