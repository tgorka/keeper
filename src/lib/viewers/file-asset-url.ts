/**
 * The URL a Files-pane file's bytes arrive over (Story 45.7, FR-180, AD-65,
 * AD-74).
 *
 * **The coordinates did not match, and this module is where that is admitted.**
 * The epic asks for media to open "over `keeper-recording://` with its range
 * support". That scheme is rooted at the effective recordings destination and
 * resolves by SESSION id; a Files-pane file has a PROFILE id and a
 * profile-relative path, and AD-74 says in as many words that the Files surface
 * must not reach for it. `keeper-note://` is rooted at `vault.root`, which is
 * `local_path/subfolder` and therefore narrower than the tree the pane browses.
 * So Story 45.7 added a fourth scheme with a fourth fixed root — the sync
 * profile's own — served by `keeper/src/file_protocol.rs` and contained by
 * `keeper_sync::browse::resolve`, the same function the listing uses.
 *
 * **Both halves arrive from Rust and neither is joined onto anything here.**
 * The profile id and the relative path both come from `sync_browse`. This
 * function escapes them and puts a `/` between them; it does not know what a
 * root is, which is the whole of AD-65's rule for a frontend. The shape mirrors
 * `recordingAssetUrl` and `keeper-note://`'s composer exactly.
 */

/** The scheme, spelled as `keeper_core::file_asset::SCHEME` in Rust. The two
 *  are pinned to each other by `file-asset-url-vectors.json`, which this
 *  module's test and `file_asset.rs`'s test both load. */
export const FILE_ASSET_SCHEME = "keeper-file";

/**
 * Percent-encode one path segment down to RFC 3986's unreserved set.
 *
 * **Stricter than `encodeURIComponent`, deliberately.** That leaves `!`, `'`,
 * `(`, `)` and `*` unescaped — legal sub-delims in a path segment, and
 * therefore a question about what a webview normalises before the request
 * reaches the handler. Escaping them costs nothing and removes the question.
 * The set that survives is exactly `keeper/src/notes_vault.rs::asset_url`'s, so
 * keeper has one spelling for an asset URL rather than one per scheme.
 *
 * `-`, `.`, `_` and `~` stay legible: without them every ordinary filename
 * turns into `a%2Db%2Epng` in the DOM and in every log line that quotes it.
 */
function segment(raw: string): string {
  return encodeURIComponent(raw).replace(
    /[!'()*]/g,
    (character) => `%${character.charCodeAt(0).toString(16).toUpperCase()}`,
  );
}

/**
 * The `keeper-file://` URL for one file inside one sync profile.
 *
 * **Total, synchronous, and it does not check anything.** A URL comes back for
 * every input, including a path the handler will refuse — a `.` or `..`
 * segment is encoded whole as `%2E`/`%2E%2E` rather than dropped, so a
 * traversal attempt reaches the log as visible text instead of as a path that
 * already collapsed before anyone could see it. The refusal is Rust's, on
 * resolution, where it can be true rather than merely likely.
 *
 * Segment by segment, so a `/` stays a separator and a space, a `#` or a `?`
 * cannot end the path.
 */
export function fileAssetUrl(profileId: string, relativePath: string): string {
  const path = relativePath
    .split("/")
    // `.replace(/\./g)` rather than `replaceAll`: TypeScript narrows `part` to
    // the literal union here, and this project's lib target does not put
    // `replaceAll` on a string literal type.
    .map((part) => (part === "." || part === ".." ? part.replace(/\./g, "%2E") : segment(part)))
    .join("/");
  return `${FILE_ASSET_SCHEME}://${segment(profileId)}/${path}`;
}
