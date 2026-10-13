repository: https://github.com/scgreenhalgh/nanokvm-mcp — `API_REFERENCE.md` only
commit: 36f416e300973b77850c45629bbf9b054ddb2aea (2025-12-21, "Sync from private repository - screenshot optimization")
licence: written from documentation; no upstream code is copied
copyright: the repository has no LICENSE file and names no holder; only its README says MIT (badge and `## License`), and `API_REFERENCE.md` says it was "reverse-engineered from the NanoKVM GitHub repository" (sipeed/NanoKVM, GPL-3.0). So, under ruling R152 (codemap Q12 (b)), its prose and tables were read and none of its code: the Python package `nanokvm_mcp/` and its tests were not opened, nor anything of sipeed/NanoKVM or sipeed/NanoKVM-Go (both GPL-3.0)
files read: `API_REFERENCE.md` — its Authentication, HID Control (paste), WebSocket HID Protocol, Video Streaming (MJPEG) and Power Control (GPIO) sections, as prose, tables and the JSON examples; the USB HID Usage Tables it links (keyboard page 0x07) for the keycodes it does not list
ported: the keyboard frame `[1, keycode, ctrl, shift, alt, meta]` with each modifier's left and right values; the mouse frame `[2, event, button, x, y]` with its five events and three buttons; the absolute-coordinate formula `int(px / size · 0x7FFE) + 1` (1…32767); the paste body `{content, langue}` and its 1024-character bound; the GPIO body `{type, duration}` and its two common durations; the login body and its password field `base64(IV ‖ AES-256-CBC(password))` under the published key zero-padded to 32 bytes; the paths `/api/auth/login`, `/api/ws`, `/api/stream/mjpeg`, `/api/hid/paste`, `/api/vm/gpio` and the cookie `nano-kvm-token`; a screenshot as the first whole JPEG (`FF D8` … `FF D9`) of the MJPEG stream — each as behaviour, from the document's words and examples
not ported: the H.264 and WebRTC streams, frame detection, storage and virtual media, Wake-on-LAN, Wi-Fi, system settings (SSH, TLS, hostname, OLED, swap), HDMI control, HID mode and reset, the SSH `/dev/hidg0` path, and every MCP tool of the reference's own server
changed: Python examples to Rust, written without the source. The IV is the caller's (the reference draws it from `os.urandom`), so the module stays pure and its encryption is tested against fixed vectors that `openssl enc -aes-256-cbc` made from the same recipe. Characters are typed through US-layout keycodes with Shift where the HID table puts them; a character no single key types is the caller's to refuse
revisit: when the reference gains a LICENSE (then (a): `licence: MIT`), when a classic NanoKVM is on the network (96.5's operator checks: a frame that differs from a device's behaviour is fixed here and the vector that held it is changed), or when NanoKVM firmware ≥ 2.3.1 changes the login (CVE-2026-32296)

# nanokvm

`keeper-ported::nanokvm` is how keeper speaks to a classic NanoKVM (story 96.5, AD-409): the
frames `keeper-agent::kvm` sends over the device's WebSocket, the bodies it posts, and the login it
makes. Everything with a network is keeper-agent's; this module only encodes.

The vectors in `mod.rs`'s tests are the reference's own examples (`[1, 40, 0, 0, 0, 0]` for
Enter, `[1, 6, 1, 0, 0, 0]` for Ctrl+C, `[2, 1, 0, 16384, 16384]` for a press at the centre, …)
plus each modifier on each side, the corners and the centre of a 1920×1080 snapshot, and two
login encryptions made outside Rust:

    KEY=6e616e6f6b766d2d7369706565642d3230323400000000000000000000000000
    printf hunter2 | openssl enc -aes-256-cbc -K $KEY -iv 000102030405060708090a0b0c0d0e0f
