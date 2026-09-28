//! The Swift bridge's C ABI (`swift/FluidAudioBridge.swift`).
//!
//! Status-returning calls answer 0 on success; otherwise `out_error` holds a
//! strdup'd message. Every string the bridge hands out is released with
//! [`fluidaudio_free_string`].

use std::ffi::{c_char, c_void};

pub type SampleSink = extern "C" fn(context: *mut c_void, data: *const f32, len: usize);

extern "C" {
    pub fn fluidaudio_engine_create() -> *mut c_void;
    pub fn fluidaudio_engine_destroy(handle: *mut c_void);
    pub fn fluidaudio_free_string(text: *mut c_char);

    pub fn fluidaudio_load_asr(
        handle: *mut c_void,
        dir: *const c_char,
        out_error: *mut *mut c_char,
    ) -> i32;
    pub fn fluidaudio_transcribe(
        handle: *mut c_void,
        samples: *const f32,
        count: usize,
        language: *const c_char,
        out_json: *mut *mut c_char,
        out_error: *mut *mut c_char,
    ) -> i32;

    pub fn fluidaudio_load_diarizer(
        handle: *mut c_void,
        dir: *const c_char,
        out_error: *mut *mut c_char,
    ) -> i32;
    pub fn fluidaudio_diarize(
        handle: *mut c_void,
        samples: *const f32,
        count: usize,
        out_json: *mut *mut c_char,
        out_error: *mut *mut c_char,
    ) -> i32;
    pub fn fluidaudio_embed(
        handle: *mut c_void,
        samples: *const f32,
        count: usize,
        out_json: *mut *mut c_char,
        out_error: *mut *mut c_char,
    ) -> i32;

    pub fn fluidaudio_audio_tracks(
        media: *const c_char,
        out_json: *mut *mut c_char,
        out_error: *mut *mut c_char,
    ) -> i32;
    /// `track < 0` mixes every audio track; `start < 0` decodes the whole file.
    pub fn fluidaudio_decode(
        media: *const c_char,
        track: i32,
        start: f64,
        end: f64,
        context: *mut c_void,
        sink: SampleSink,
        out_error: *mut *mut c_char,
    ) -> i32;
}
