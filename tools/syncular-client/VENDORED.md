# syncular-client — keeper's vendored copy

This directory is `syncular-client` **0.97.0**, the Rust client of the Syncular
sync engine, exactly as crates.io serves it, plus one line. keeper uses it
through `[patch.crates-io]` in `src-tauri/Cargo.toml` (story 115.6).

- upstream: <https://github.com/syncular/syncular> (`rust/crates/client`)
- copied from: the crates.io package `syncular-client-0.97.0.crate`,
  sha256 `1329fe09caed1fc82f22b3a0d808a5de4ed4c3e0f6a783bec1fa0b9503a14621`
- licence: Apache-2.0 (the package's `license` field; the package ships no
  LICENSE or NOTICE file)

## The change

```diff
--- syncular-client-0.97.0/src/client.rs
+++ tools/syncular-client/src/client.rs
@@ -716,7 +716,7 @@
             .iter()
             .find(|column| column.name == name)
             .ok_or(())?;
-        Ok((quote_ident(name), column.ty == ColumnType::Integer))
+        Ok::<_, ()>((quote_ident(name), column.ty == ColumnType::Integer))
     };
```

Why: the `column` closure applies `?` to two `Result<_, ()>` values and never
names its own error type, so rustc infers `E` from `E: From<()>`. The crate on
its own has one candidate and compiles. keeper's dependency graph adds
`aws-lc-rs` (through rustls under reqwest 0.13), whose `KeyRejected` and
`Unspecified` implement `From<()>` beside `serde_json::Value`, and the crate
then fails with E0282/E0283 at that line. The annotation states what the code
already means.

## Removal

Delete this directory and its `[patch.crates-io]` entry, then run
`cargo update -p syncular-client`, as soon as a Syncular release names the
closure's error type (or otherwise compiles in keeper's graph).
