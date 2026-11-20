# matrix-sdk-sqlite — keeper's vendored copy

This directory is `matrix-sdk-sqlite` **0.19.1**, the SQLite state, crypto and
event-cache stores of matrix-rust-sdk, exactly as crates.io serves it, with
one requirement changed. keeper uses it through `[patch.crates-io]` in
`src-tauri/Cargo.toml` (story 115.6).

- upstream: <https://github.com/matrix-org/matrix-rust-sdk> (`crates/matrix-sdk-sqlite`)
- copied from: the crates.io package `matrix-sdk-sqlite-0.19.1.crate`,
  sha256 `8d4271e06d52a17bc6ec39b5a2985b65dcced737969fac3678967783656edb34`
- licence: Apache-2.0 (the package's `license` field; the package ships no
  LICENSE or NOTICE file); its text is the repository's root `LICENSE`

## The change

```diff
--- matrix-sdk-sqlite-0.19.1/Cargo.toml
+++ tools/matrix-sdk-sqlite/Cargo.toml
@@ -131,7 +131,7 @@
 ]
 
 [dependencies.rusqlite]
-version = "0.40.2"
+version = "0.41"
 features = [
     "limits",
     "fallible_uint",
```

No source file changes: the crate compiles unchanged against rusqlite 0.41.

Why: every `syncular-client` from 0.58.0 requires rusqlite 0.41
(`libsqlite3-sys` 0.39), and matrix-sdk 0.19.1, the newest release, requires
rusqlite 0.40 (`libsqlite3-sys` 0.38). Both `libsqlite3-sys` versions declare
`links = "sqlite3"`, so cargo cannot resolve the two in one graph, even with
matrix-sdk made optional. With this copy keeper links one SQLite and keeps
Matrix on, its on-disk store unchanged.

## Removal

Delete this directory and its `[patch.crates-io]` entry, then run
`cargo update -p matrix-sdk-sqlite`, as soon as a matrix-sdk release whose
`matrix-sdk-sqlite` requires rusqlite 0.41 exists (matrix-rust-sdk `main`
already does).
