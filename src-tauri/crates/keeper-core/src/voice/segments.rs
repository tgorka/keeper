//! Joining a segmented recogniser's results into one utterance (Epic 98,
//! story 98.4 #5; AD-414, D-36).
//!
//! Android's on-device recogniser, fed keeper's own echo-cancelled capture
//! (`EXTRA_AUDIO_SOURCE`), runs one *segmented session*: it reports the
//! words of the segment it is hearing as partial results that replace each
//! other, closes that segment with a segment result where it heard a pause,
//! and goes on with the next segment's partials. No single result is the
//! utterance. The turn, though, reads one transcript
//! ([`super::TurnEvent::PartialHeard`], [`super::TurnEvent::FinalHeard`]),
//! so the port hands every result to [`Segments`] and reports
//! [`Segments::utterance`] — the finished segments in order, then the
//! segment being heard.
//!
//! # The rules
//!
//! - **A partial replaces the partial before it**, never appends: each one
//!   is the recogniser's whole guess for the segment so far.
//! - **A segment result closes the segment.** It replaces that segment's
//!   last partial, and the next partial starts a new segment after it — so
//!   a segment's words are kept once, and the next segment's first words
//!   never overwrite them.
//! - **Nothing is taken off at a join.** A segment's results carry only that
//!   segment's words (`RecognitionListener.onSegmentResults`), so a word
//!   that ends one segment and starts the next was said twice ("no" … "no
//!   more") and is kept twice; guessing at overlaps would drop real words.
//! - **A blank result is nothing**: a segment that heard only silence
//!   closes without adding a space ([`Segments::utterance`] skips it).
//! - **Only the last [`KEEP_CLOSED`] segments are kept.** With the phrase
//!   armed a session runs for as long as keeper is in front, and every
//!   segment the room produces closes into it; the utterance is what the
//!   turn reads on every partial, so it is bounded here rather than left to
//!   grow with the day. A question is far shorter than the window, and the
//!   phrase is matched on the words just heard.
//!
//! The port starts a fresh [`Segments`] wherever it starts a fresh
//! recogniser session (`keeper_core::voice::android::Sessions`): opening
//! the microphone, before speaking and on continuing a paused answer while
//! the microphone is open — where the Apple ports start a fresh recognition
//! request — and once the final words are delivered.

/// How many closed segments [`Segments`] keeps, the oldest dropped first:
/// enough for a long spoken question, few enough that an armed day of room
/// noise costs nothing.
pub const KEEP_CLOSED: usize = 16;

/// The results of one segmented recognition session, as one utterance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Segments {
    /// Every closed segment's words, in order, each once (blank for a
    /// segment that heard only silence).
    closed: Vec<String>,
    /// The segment being heard: its latest partial, if any.
    open: String,
}

impl Segments {
    /// A session that has heard nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The recogniser's latest guess for the segment it is hearing. Returns
    /// the utterance so far.
    pub fn partial(&mut self, text: &str) -> String {
        self.open = words(text);
        self.utterance()
    }

    /// The recogniser closed the segment it was hearing with `text`. Returns
    /// the utterance so far, of at most [`KEEP_CLOSED`] segments.
    pub fn segment(&mut self, text: &str) -> String {
        self.closed.push(words(text));
        if self.closed.len() > KEEP_CLOSED {
            let over = self.closed.len() - KEEP_CLOSED;
            self.closed.drain(..over);
        }
        self.open.clear();
        self.utterance()
    }

    /// Every closed segment, then the one being heard, separated by one
    /// space each.
    pub fn utterance(&self) -> String {
        let mut out = String::new();
        for words in self.closed.iter().chain(std::iter::once(&self.open)) {
            if words.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(words);
        }
        out
    }
}

/// `text`'s words, single-spaced.
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
