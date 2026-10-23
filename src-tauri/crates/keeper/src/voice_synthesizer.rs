//! What both Apple voice ports do to `AVSpeechSynthesizer` beyond speaking
//! and stopping (AD-411, story 97.3): pause it mid-word, go on from there,
//! and hear each utterance read to its end.
//!
//! Whether to pause, resume or stop is `keeper_core::voice`'s
//! (`Effect::PauseSpeaking`, `Effect::ContinueSpeaking`); how far the person
//! heard is `keeper_core::hq::spoken::HeardMark`'s count of the
//! utterances this file reports finished. A delegate, not the ports'
//! `isSpeaking` poll, reports them: the poll sees only the whole queue
//! drain, and a sentence the synthesiser was cut off in must not count
//! (Q3). `didCancelSpeechUtterance:` is not implemented — a cancelled
//! utterance is one the person did not hear to its end.
//!
//! # `unsafe`
//!
//! One `#[allow(unsafe_code)]` per concern, each with its `// SAFETY:`
//! comment and its row in the audit inventory in
//! `docs/constraints-and-limitations.md`: the delegate class, setting it,
//! pausing, continuing.
//!
//! # Not compiled where it was written
//!
//! Written on a Linux host; its first compile is CI's macOS job and iOS
//! check, and `check:rust:macos` on hesperia.

#![cfg(any(target_os = "ios", target_os = "macos"))]

use keeper_core::voice::EventSink;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_avf_audio::{AVSpeechBoundary, AVSpeechSynthesizer};

/// The delegate class, in a module of its own so the one
/// `#[allow(unsafe_code)]` covers exactly the class definition —
/// `define_class!` is `unsafe impl`s by construction (the
/// `web_auth_apple.rs` precedent).
#[allow(unsafe_code)]
mod class {
    use keeper_core::voice::{EventSink, TurnEvent};
    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol};
    use objc2::{define_class, msg_send, AnyThread, DefinedClass};
    use objc2_avf_audio::{AVSpeechSynthesizer, AVSpeechSynthesizerDelegate, AVSpeechUtterance};

    define_class!(
        // SAFETY: `NSObject` has no subclassing requirements, and
        // `SpeechDelegate` does not implement `Drop`. Any thread: the
        // protocol is `Send + Sync`, the synthesiser calls its delegate on a
        // queue of its own, and the only ivar is the port's sink, which is
        // `Send + Sync` and never touches a framework object.
        #[unsafe(super(NSObject))]
        #[ivars = EventSink]
        pub struct SpeechDelegate;

        unsafe impl NSObjectProtocol for SpeechDelegate {}

        // SAFETY: the one method implemented has the signature AVFAudio
        // declares — `- (void)speechSynthesizer:(AVSpeechSynthesizer *)
        // synthesizer didFinishSpeechUtterance:(AVSpeechUtterance *)
        // utterance` — and only hands the sink an event; it reads neither
        // argument.
        unsafe impl AVSpeechSynthesizerDelegate for SpeechDelegate {
            #[unsafe(method(speechSynthesizer:didFinishSpeechUtterance:))]
            fn did_finish(
                &self,
                _synthesizer: &AVSpeechSynthesizer,
                _utterance: &AVSpeechUtterance,
            ) {
                (self.ivars())(TurnEvent::SentenceSpoken);
            }
        }
    );

    impl SpeechDelegate {
        /// A delegate reporting into `sink`.
        pub fn new(sink: EventSink) -> Retained<Self> {
            let this = Self::alloc().set_ivars(sink);
            // SAFETY: `init` is `NSObject`'s designated initializer, and the
            // ivars were set on the allocation above, as `define_class!`
            // requires before it is sent.
            unsafe { msg_send![super(this), init] }
        }
    }
}

pub use class::SpeechDelegate;

/// A synthesiser with a delegate reporting each finished utterance into
/// `sink`. The delegate property is weak: the caller keeps the delegate for
/// as long as it keeps the synthesiser.
pub fn with_delegate(
    synthesizer: &AVSpeechSynthesizer,
    sink: EventSink,
) -> Retained<SpeechDelegate> {
    let delegate = SpeechDelegate::new(sink);
    set_delegate(synthesizer, &delegate);
    delegate
}

/// `setDelegate:`.
#[allow(unsafe_code)]
fn set_delegate(synthesizer: &AVSpeechSynthesizer, delegate: &SpeechDelegate) {
    // SAFETY: a weak property set to an object implementing the protocol;
    // the caller retains the object for the synthesiser's lifetime
    // (`with_delegate`'s contract), so the weak reference never dangles
    // while the synthesiser can call it.
    unsafe { synthesizer.setDelegate(Some(ProtocolObject::from_ref(delegate))) };
}

/// `pauseSpeakingAtBoundary:AVSpeechBoundaryImmediate` — mid-word, keeping
/// the utterance and the queue.
#[allow(unsafe_code)]
pub fn pause_speech(synthesizer: &AVSpeechSynthesizer) {
    // SAFETY: a documented enum value on the retained synthesiser; returns
    // whether anything was paused, which is not needed.
    let _ = unsafe { synthesizer.pauseSpeakingAtBoundary(AVSpeechBoundary::Immediate) };
}

/// `continueSpeaking` — from where the pause left off.
#[allow(unsafe_code)]
pub fn continue_speech(synthesizer: &AVSpeechSynthesizer) {
    // SAFETY: a method without arguments on the retained synthesiser;
    // returns whether anything resumed, which is not needed: with nothing
    // left, the port's poll reports the answer's end.
    let _ = unsafe { synthesizer.continueSpeaking() };
}
