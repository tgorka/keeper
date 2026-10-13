//! The classic NanoKVM's protocol, encoded (AD-396, ruling R21): its HID
//! frames over `/api/ws`, the mapping of a snapshot's pixels onto its
//! absolute coordinates, its paste and power bodies, its login's password
//! obfuscation, and the first JPEG of its MJPEG stream.
//!
//! Written from the prose and tables of the MIT community reference
//! `scgreenhalgh/nanokvm-mcp`'s `API_REFERENCE.md` and the USB HID usage
//! tables it links; nothing of the NanoKVM firmware (GPL-3.0) was read as
//! code. `UPSTREAM.md` says what was read. Pure: no network, no clock, no
//! randomness — the login's IV is the caller's.

use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use base64::Engine;
use serde_json::json;

/// `POST`: the login; its answer sets [`TOKEN_COOKIE`].
pub const LOGIN_PATH: &str = "/api/auth/login";
/// The WebSocket that takes keyboard and mouse frames.
pub const WS_PATH: &str = "/api/ws";
/// `GET`: the screen as `multipart/x-mixed-replace` JPEG frames.
pub const MJPEG_PATH: &str = "/api/stream/mjpeg";
/// `POST`: text typed by the device itself, one keypress per character.
pub const PASTE_PATH: &str = "/api/hid/paste";
/// `POST`: the target's power or reset button pressed for a duration.
pub const GPIO_PATH: &str = "/api/vm/gpio";
/// The cookie every endpoint but the login is authenticated by.
pub const TOKEN_COOKIE: &str = "nano-kvm-token";
/// The most characters one paste takes.
pub const PASTE_MAX: usize = 1024;
/// A short press of the power button: on, or a clean off.
pub const PRESS_MS: u32 = 800;
/// A long press: forced off.
pub const HOLD_MS: u32 = 5000;

/// The key the login's password is encrypted under: the published string,
/// zero-padded to 32 bytes.
const LOGIN_KEY: &[u8] = b"nanokvm-sipeed-2024";

/// Which of a modifier's two keys is held, if any.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Side {
    /// Not held.
    #[default]
    Off,
    Left,
    Right,
}

impl Side {
    /// The frame's value for a modifier whose left key is `left` and whose
    /// right key is `left << 4`.
    fn value(self, left: i32) -> i32 {
        match self {
            Side::Off => 0,
            Side::Left => left,
            Side::Right => left << 4,
        }
    }
}

/// The modifiers held with a key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: Side,
    pub shift: Side,
    pub alt: Side,
    pub meta: Side,
}

/// A keyboard frame: `[1, keycode, ctrl, shift, alt, meta]`.
pub fn key(keycode: u8, modifiers: Modifiers) -> [i32; 6] {
    [
        1,
        i32::from(keycode),
        modifiers.ctrl.value(1),
        modifiers.shift.value(2),
        modifiers.alt.value(4),
        modifiers.meta.value(8),
    ]
}

/// Every key released.
pub const KEY_RELEASE: [i32; 6] = [1, 0, 0, 0, 0, 0];

/// A mouse frame's event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEvent {
    Down = 1,
    Up = 2,
    MoveAbsolute = 3,
    Scroll = 4,
    MoveRelative = 5,
}

/// A mouse button; `Left` is also "none".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left = 0,
    Middle = 1,
    Right = 2,
}

/// A mouse frame: `[2, event, button, x, y]` — absolute coordinates for a
/// press or an absolute move, a delta for a relative move or a scroll.
pub fn mouse(event: MouseEvent, button: Button, x: i32, y: i32) -> [i32; 5] {
    [2, event as i32, button as i32, x, y]
}

/// Where pixel `px` of a snapshot `size` pixels across lies in absolute
/// coordinates: `int(px / size · 0x7FFE) + 1`, so 0 is 1. A snapshot's
/// pixels are `0..size`; `None` for an empty snapshot or a pixel outside
/// it — the far edge included, which no pixel of the frame is.
pub fn absolute(px: u32, size: u32) -> Option<i32> {
    if px >= size {
        return None;
    }
    let scaled = u64::from(px) * 0x7FFE / u64::from(size);
    i32::try_from(scaled).ok().map(|scaled| scaled + 1)
}

/// A frame as the WebSocket carries it: a JSON array.
pub fn frame_text(frame: &[i32]) -> String {
    serde_json::to_string(frame).unwrap_or_default()
}

/// The US-layout key that types `c`, and whether Shift is held for it;
/// `None` for a character no single key types.
pub fn char_key(c: char) -> Option<(u8, bool)> {
    const SHIFTED_DIGITS: &str = ")!@#$%^&*(";
    const PUNCTUATION: [(char, char, u8); 11] = [
        ('-', '_', 0x2D),
        ('=', '+', 0x2E),
        ('[', '{', 0x2F),
        (']', '}', 0x30),
        ('\\', '|', 0x31),
        (';', ':', 0x33),
        ('\'', '"', 0x34),
        ('`', '~', 0x35),
        (',', '<', 0x36),
        ('.', '>', 0x37),
        ('/', '?', 0x38),
    ];
    let digit = |d: u32| if d == 0 { 0x27 } else { 0x1D + d as u8 };
    match c {
        'a'..='z' => Some((0x04 + (c as u8 - b'a'), false)),
        'A'..='Z' => Some((0x04 + (c as u8 - b'A'), true)),
        '0'..='9' => c.to_digit(10).map(|d| (digit(d), false)),
        '\n' => Some((0x28, false)),
        '\t' => Some((0x2B, false)),
        ' ' => Some((0x2C, false)),
        _ => SHIFTED_DIGITS
            .find(c)
            .map(|d| (digit(d as u32), true))
            .or_else(|| {
                PUNCTUATION.iter().find_map(|&(plain, shifted, code)| {
                    (c == plain)
                        .then_some((code, false))
                        .or((c == shifted).then_some((code, true)))
                })
            }),
    }
}

/// The key a name stands for: the reference's named keys and the rest of
/// the HID table's navigation keys; `None` for any other name.
pub fn named_key(name: &str) -> Option<u8> {
    let code = match name {
        "enter" => 0x28,
        "escape" => 0x29,
        "backspace" => 0x2A,
        "tab" => 0x2B,
        "space" => 0x2C,
        "insert" => 0x49,
        "home" => 0x4A,
        "pageup" => 0x4B,
        "delete" => 0x4C,
        "end" => 0x4D,
        "pagedown" => 0x4E,
        "right" => 0x4F,
        "left" => 0x50,
        "down" => 0x51,
        "up" => 0x52,
        _ => {
            let n: u8 = name.strip_prefix('f')?.parse().ok()?;
            return (1..=12).contains(&n).then(|| 0x39 + n);
        }
    };
    Some(code)
}

/// The paste body for `text` (US layout); `Err` when it is longer than
/// [`PASTE_MAX`] characters.
pub fn paste_body(text: &str) -> Result<String, String> {
    let count = text.chars().count();
    if count > PASTE_MAX {
        return Err(format!(
            "a paste is at most {PASTE_MAX} characters; this one is {count}"
        ));
    }
    Ok(json!({ "content": text, "langue": "" }).to_string())
}

/// Which GPIO button a power action presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerButton {
    Power,
    Reset,
}

/// The GPIO body pressing `button` for `duration_ms`.
pub fn gpio_body(button: PowerButton, duration_ms: u32) -> String {
    let kind = match button {
        PowerButton::Power => "power",
        PowerButton::Reset => "reset",
    };
    json!({ "type": kind, "duration": duration_ms }).to_string()
}

/// The login's password field: `base64(iv ‖ AES-256-CBC(password))`, PKCS#7
/// padded, under the published key — an obfuscation, not a secret: anyone
/// who sees it can undo it, so it travels only inside TLS.
pub fn login_password(password: &str, iv: [u8; 16]) -> String {
    let mut key = [0u8; 32];
    key[..LOGIN_KEY.len()].copy_from_slice(LOGIN_KEY);
    let sealed = cbc::Encryptor::<aes::Aes256>::new(&key.into(), &iv.into())
        .encrypt_padded_vec_mut::<Pkcs7>(password.as_bytes());
    let mut out = Vec::with_capacity(iv.len() + sealed.len());
    out.extend_from_slice(&iv);
    out.extend_from_slice(&sealed);
    base64::engine::general_purpose::STANDARD.encode(out)
}

/// The login's body for `username` and an encrypted `password`.
pub fn login_body(username: &str, password: &str) -> String {
    json!({ "username": username, "password": password }).to_string()
}

/// The first whole JPEG of an MJPEG stream, found as its bytes arrive: from
/// its start marker `FF D8` to the first end marker `FF D9` after it, as
/// the reference takes a screenshot. Each [`JpegCut::find`] scans only the
/// bytes that arrived since the last — one pass over the stream however it
/// is cut, a marker split between two reads included.
#[derive(Debug, Default)]
pub struct JpegCut {
    /// Where the start marker is, once found.
    start: Option<usize>,
    /// The first two-byte window not yet looked at.
    next: usize,
}

impl JpegCut {
    /// Where the first whole JPEG of `stream` — the same stream, grown
    /// since the last call — lies; `None` until one is whole.
    pub fn find(&mut self, stream: &[u8]) -> Option<std::ops::Range<usize>> {
        let marker = |at: usize, second: u8| stream[at] == 0xFF && stream[at + 1] == second;
        while self.next + 1 < stream.len() {
            let at = self.next;
            match self.start {
                None if marker(at, 0xD8) => {
                    self.start = Some(at);
                    self.next = at + 2;
                }
                Some(start) if marker(at, 0xD9) => return Some(start..at + 2),
                _ => self.next = at + 1,
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(ctrl: Side, shift: Side, alt: Side, meta: Side) -> Modifiers {
        Modifiers {
            ctrl,
            shift,
            alt,
            meta,
        }
    }

    /// 96.5 #1: every frame shape `API_REFERENCE.md` shows, as it shows it.
    #[test]
    fn nanokvm_frames_match_the_reference() {
        use Side::{Left, Off, Right};
        // Keyboard: Enter, Ctrl+C, the release, each modifier on each side.
        assert_eq!(key(40, Modifiers::default()), [1, 40, 0, 0, 0, 0]);
        assert_eq!(key(6, held(Left, Off, Off, Off)), [1, 6, 1, 0, 0, 0]);
        assert_eq!(KEY_RELEASE, [1, 0, 0, 0, 0, 0]);
        assert_eq!(key(4, held(Right, Off, Off, Off)), [1, 4, 16, 0, 0, 0]);
        assert_eq!(key(4, held(Off, Left, Off, Off)), [1, 4, 0, 2, 0, 0]);
        assert_eq!(key(4, held(Off, Right, Off, Off)), [1, 4, 0, 32, 0, 0]);
        assert_eq!(key(4, held(Off, Off, Left, Off)), [1, 4, 0, 0, 4, 0]);
        assert_eq!(key(4, held(Off, Off, Right, Off)), [1, 4, 0, 0, 64, 0]);
        assert_eq!(key(4, held(Off, Off, Off, Left)), [1, 4, 0, 0, 0, 8]);
        assert_eq!(key(4, held(Off, Off, Off, Right)), [1, 4, 0, 0, 0, 128]);
        assert_eq!(key(0x4C, held(Left, Off, Left, Off)), [1, 0x4C, 1, 0, 4, 0]);
        assert_eq!(frame_text(&key(40, Modifiers::default())), "[1,40,0,0,0,0]");

        // Mouse: a click at the centre of 1920×1080, a right click, a move,
        // a scroll each way, a relative move.
        let (x, y) = (absolute(960, 1920), absolute(540, 1080));
        assert_eq!((x, y), (Some(16384), Some(16384)));
        let (x, y) = (x.unwrap_or_default(), y.unwrap_or_default());
        assert_eq!(
            mouse(MouseEvent::Down, Button::Left, x, y),
            [2, 1, 0, 16384, 16384]
        );
        assert_eq!(mouse(MouseEvent::Up, Button::Left, 0, 0), [2, 2, 0, 0, 0]);
        assert_eq!(
            mouse(MouseEvent::Down, Button::Right, 0, 0),
            [2, 1, 2, 0, 0]
        );
        assert_eq!(
            mouse(MouseEvent::MoveAbsolute, Button::Left, x, y),
            [2, 3, 0, 16384, 16384]
        );
        assert_eq!(
            mouse(MouseEvent::Scroll, Button::Left, 0, 1),
            [2, 4, 0, 0, 1]
        );
        assert_eq!(
            mouse(MouseEvent::Scroll, Button::Left, 0, -1),
            [2, 4, 0, 0, -1]
        );
        assert_eq!(
            mouse(MouseEvent::MoveRelative, Button::Middle, -5, 7),
            [2, 5, 1, -5, 7]
        );
        assert_eq!(frame_text(&[2, 4, 0, 0, -1]), "[2,4,0,0,-1]");

        // The corners: (1, 1) top-left; the last pixel of 1920 × 1080, as
        // `int()` truncates (1919 / 1920 · 32766 = 32748.93…, plus one).
        // The far edge is no pixel of the frame (R96K1-05).
        assert_eq!((absolute(0, 1920), absolute(0, 1080)), (Some(1), Some(1)));
        assert_eq!(
            (absolute(1919, 1920), absolute(1079, 1080)),
            (Some(32749), Some(32736))
        );
        assert_eq!(absolute(1920, 1920), None);
        assert_eq!(absolute(1921, 1920), None);
        assert_eq!(absolute(0, 0), None);

        // The table's keycodes, and the characters typed through them.
        assert_eq!(char_key('a'), Some((0x04, false)));
        assert_eq!(char_key('Z'), Some((0x1D, true)));
        assert_eq!(char_key('1'), Some((0x1E, false)));
        assert_eq!(char_key('0'), Some((0x27, false)));
        assert_eq!(char_key('!'), Some((0x1E, true)));
        assert_eq!(char_key(')'), Some((0x27, true)));
        assert_eq!(char_key(' '), Some((0x2C, false)));
        assert_eq!(char_key('\n'), Some((0x28, false)));
        assert_eq!(char_key('?'), Some((0x38, true)));
        assert_eq!(char_key('/'), Some((0x38, false)));
        assert_eq!(char_key('ł'), None);
        for (name, code) in [
            ("enter", 0x28),
            ("escape", 0x29),
            ("backspace", 0x2A),
            ("tab", 0x2B),
            ("space", 0x2C),
            ("up", 0x52),
            ("down", 0x51),
            ("left", 0x50),
            ("right", 0x4F),
            ("delete", 0x4C),
            ("insert", 0x49),
            ("f1", 0x3A),
            ("f12", 0x45),
        ] {
            assert_eq!(named_key(name), Some(code), "{name}");
        }
        assert_eq!(named_key("f13"), None);
        assert_eq!(named_key("f0"), None);
        assert_eq!(named_key("hyper"), None);
    }

    /// The paste, power and login bodies, and the login's encryption against
    /// vectors `openssl enc -aes-256-cbc` made from the reference's recipe
    /// (key `nanokvm-sipeed-2024` zero-padded to 32 bytes, IV 00…0f).
    #[test]
    fn nanokvm_bodies_match_the_reference() {
        let body: serde_json::Value =
            serde_json::from_str(&paste_body("Hello World").expect("short")).expect("json");
        assert_eq!(body, json!({ "content": "Hello World", "langue": "" }));
        assert!(paste_body(&"x".repeat(PASTE_MAX)).is_ok());
        assert!(paste_body(&"x".repeat(PASTE_MAX + 1)).is_err());
        let gpio: serde_json::Value =
            serde_json::from_str(&gpio_body(PowerButton::Power, PRESS_MS)).expect("json");
        assert_eq!(gpio, json!({ "type": "power", "duration": 800 }));
        let gpio: serde_json::Value =
            serde_json::from_str(&gpio_body(PowerButton::Reset, HOLD_MS)).expect("json");
        assert_eq!(gpio, json!({ "type": "reset", "duration": 5000 }));

        let iv: [u8; 16] = std::array::from_fn(|i| i as u8);
        assert_eq!(
            login_password("hunter2", iv),
            "AAECAwQFBgcICQoLDA0OD7vc2PTT3keSkxfwJqVNxVo="
        );
        // A whole block of password gains a whole block of padding.
        assert_eq!(
            login_password("sixteen-chars-pw", iv),
            "AAECAwQFBgcICQoLDA0OD1Rq3ZA1zvfYx5pKCRLcgodRon8VpILwEyKdiwH0ADN8"
        );
        let login: serde_json::Value =
            serde_json::from_str(&login_body("admin", "AAEC")).expect("json");
        assert_eq!(login, json!({ "username": "admin", "password": "AAEC" }));
    }

    /// The first whole JPEG, however the stream arrives: whole, or a byte
    /// at a time — each marker split between two reads.
    #[test]
    fn the_first_whole_jpeg_is_cut_from_the_stream() {
        let cut = |stream: &[u8], step: usize| {
            let mut finder = JpegCut::default();
            (step..=stream.len())
                .step_by(step)
                .chain([stream.len()])
                .find_map(|end| finder.find(&stream[..end]))
                .map(|range| stream[range].to_vec())
        };
        let stream = b"--frame\r\nContent-Type: image/jpeg\r\n\r\n\xFF\xD8ab\xFF\xD9\r\n--frame\r\n\xFF\xD8cd\xFF\xD9";
        for step in [1, 2, 3, stream.len()] {
            assert_eq!(
                cut(stream, step),
                Some(b"\xFF\xD8ab\xFF\xD9".to_vec()),
                "{step}"
            );
            // An end marker before any start is not one.
            assert_eq!(
                cut(b"\xFF\xD9\xFF\xD8x\xFF\xD9", step.min(7)),
                Some(b"\xFF\xD8x\xFF\xD9".to_vec())
            );
        }
        assert_eq!(cut(b"--frame\r\n\xFF\xD8abc", 1), None);
    }
}
