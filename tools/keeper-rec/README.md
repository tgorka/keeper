# keeper-rec

`keeper-rec` is keeper's first-party screen-recording capture sidecar: a small,
zero-dependency SwiftPM executable that keeper spawns as a child process and
drives over an NDJSON-RPC protocol (one JSON object per line on stdio). Today it
answers the `getCapabilities` handshake and exits cleanly; the real
ScreenCaptureKit + AVFoundation capture path (full display + system audio to a
fragmented MP4) lands in Story 16.6. It links only Apple system frameworks — no
third-party dependencies, no ffmpeg — so keeper's license firewall is untouched.

## Build

```sh
bash scripts/build-keeper-rec.sh
```

This runs `swift build -c release --arch arm64` and installs the product to
`src-tauri/crates/keeper/binaries/keeper-rec-aarch64-apple-darwin`, the per-triple
name Tauri's `bundle.externalBin` resolves. Requires macOS 13+ and a Swift
toolchain; aarch64-only (no universal/lipo build).

## License

Apache-2.0. See `SPDX-License-Identifier: Apache-2.0` headers in the sources.
