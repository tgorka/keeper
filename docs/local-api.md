# Local API

keeper serves a local API over one Unix domain socket at `<data dir>/wire/wire.sock` (story 123.3):

- the macOS app: `~/Library/Application Support/dev.tgorka.keeper/wire/wire.sock`;
- `keeper-cli serve --state <dir>`: `<dir>/wire/wire.sock`.

`$XDG_RUNTIME_DIR`, `/tmp` and TCP are never used. There is one listener per data directory.

The `wire/` directory is mode `0700` and the socket mode `0600`. On every connection keeper reads the peer's uid from the kernel's credentials; a process of another user is closed with no byte read or written.

## Off by default

The setting `wire.enabled` is `0`. It is session state on this device only: no config file, no synced settings file and no other device can switch it on.

Until 123.4 adds a Settings control, it is switched on by `keeper-cli serve` at a terminal, or in a debug build by `KEEPER_WIRE_DEBUG_ON=1` (a release build ignores the variable).

## What it serves

Only the `app` reads and `describe`:

- `app.version`
- `app.modules`
- `app.health` (no sentence over the socket)
- `app.capabilities`
- `app.domains` (empty for a caller on the socket)

Every other operation answers `NOT_FOUND`.

## The envelope, `keeper.wire.v1`

One JSON object per line, ended by `\n`. Each side's first line is `hello`:

```json
{"type":"hello","wire":["keeper.wire.v1"],"client":"me/1"}
{"type":"hello","wire":"keeper.wire.v1","app_version":"0.12.0","catalogue_digest":"…","max_line_bytes":4194304,"max_in_flight":64}
```

Then the client sends `call`, `watch`, `describe` or `cancel`, each naming its call by a `call_id` (a ULID, upper case), many at once on one connection:

```json
{"type":"call","call_id":"01JD3N1X8Q2W9V4C7B6M5K0H3R","operation":"app.version","input":{}}
```

The server answers:

- `reply`: the one answer of a `call` or a `describe`, with `output` or `error`;
- `event`: one item of a `watch`, with `seq` 1, 2, 3 …;
- `end`: a stream's last frame, with `error` unless it simply finished;
- `close`: the server's last line when it closes the connection for a reason of its own.

An error carries a code (`NOT_FOUND`, `INVALID_ARGUMENT`, `REFUSED`, `UNAVAILABLE`, `CONFLICT`, `DEADLINE_EXCEEDED`, `CANCELLED`, `RESOURCE_EXHAUSTED`, `INTERNAL`) and keeper's fixed sentence for it; a module's own text is logged, never sent.

### Limits

| Limit | Value |
| --- | --- |
| A line, its `\n` included | 4 MiB |
| Calls and streams in flight per connection | 64 |
| Connections per listener | 16 |
| Calls in flight per listener | 128 |
| Events waiting to be written, per stream | 256, or 4 MiB |
| A frame not wholly written after | 10 s: the connection is closed |
| Deadline of a query / a command (also the default) | 30 s / 120 s |

## Two things a client meets

- **Keep the connection open until the answers arrive.** `printf '…' | nc -U <path>` gets no answer: the end of the client's write side ends the connection. Use `socat`, or a client that does not half-close.
- **Read promptly.** The 10 s limit is a frame's age, so a client that reads one slow stream behind other answers is closed by it before the per-stream bound fires. Read promptly, or open a second connection for a slow stream.

## A second keeper on the same data directory

keeper holds `<data dir>/keeper.lock` for its whole run, wire on or off. A second keeper process on the same data directory (a development build beside the installed app, say) opens nothing there and names the holder.

## Graph rows of an unlisted domain

A domain taken off the list keeps its graph rows until it is revoked: there is no unsubscribe yet.

## The test client

```sh
cargo run --manifest-path src-tauri/Cargo.toml -p keeper-wire-api --example wire_probe -- --data-dir <dir> app.version
```
