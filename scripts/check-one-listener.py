#!/usr/bin/env python3
"""Enforce MA-G8: one Unix socket listener and one TCP loopback listener.

In non-test Rust code under `src-tauri/crates/*/src/**/*.rs` (skipping
`tests/`, `benches/`, `examples/` directories and `#[cfg(test)]` modules):

- `UnixListener::bind` is allowed only under `src-tauri/crates/keeper-wire/src/`.
- `TcpListener::bind` is allowed only in `src-tauri/crates/keeper-core/src/org_account/loopback.rs`
  (plus named legacy/testkit exceptions below).
- `UdpSocket::bind` and abstract-namespace socket helpers
  (`from_abstract_name`, `SocketAddr::from_abstract`) are forbidden everywhere.

Set `KEEPER_LISTENER_ROOT` to override the repository root for self-tests.
"""
import os
from pathlib import Path
import re
import sys

ROOT = Path(os.environ.get("KEEPER_LISTENER_ROOT", Path(__file__).resolve().parents[1]))
CRATES_DIR = ROOT / "src-tauri" / "crates"

SKIP_DIRS = {"tests", "benches", "examples"}
ALLOWED_UNIX_PREFIX = "src-tauri/crates/keeper-wire/src/"
ALLOWED_TCP_FILES = {
    "src-tauri/crates/keeper-core/src/org_account/loopback.rs",
    # Named dev-only test-double crate (`keeper-testkit`, role `provider`, never
    # linked into shipped binaries): binds ephemeral 127.0.0.1:0 fake servers.
    "src-tauri/crates/keeper-testkit/src/lib.rs",
    "src-tauri/crates/keeper-testkit/src/store.rs",
}

CHECKS = (
    ("UnixListener::bind", re.compile(r"\bUnixListener::bind\b")),
    ("TcpListener::bind", re.compile(r"\bTcpListener::bind\b")),
    ("UdpSocket::bind", re.compile(r"\bUdpSocket::bind\b")),
    (
        "abstract-namespace",
        re.compile(r"\b(?:SocketAddr::from_abstract|from_abstract_name)\b"),
    ),
)

CFG_TEST_RE = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")
PATH_ATTR_RE = re.compile(r'#\s*\[\s*path\s*=\s*"([^"]+)"\s*\]')
OUT_OF_LINE_MOD_RE = re.compile(
    r"^(?:\s*(?:pub(?:\s*\([^)]*\))?\s+)?)mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;"
)
INLINE_MOD_RE = re.compile(
    r"^(?:\s*(?:pub(?:\s*\([^)]*\))?\s+)?)mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{"
)


def strip_comments_and_strings(source: str) -> str:
    """Replace comments and string/char literals with spaces, preserving newlines."""
    out: list[str] = []
    n = len(source)
    i = 0
    while i < n:
        ch = source[i]
        # Line comment
        if ch == "/" and i + 1 < n and source[i + 1] == "/":
            while i < n and source[i] != "\n":
                out.append(" ")
                i += 1
            continue
        # Nested block comment
        if ch == "/" and i + 1 < n and source[i + 1] == "*":
            depth = 1
            out.append("  ")
            i += 2
            while i < n and depth > 0:
                if source[i] == "/" and i + 1 < n and source[i + 1] == "*":
                    depth += 1
                    out.append("  ")
                    i += 2
                elif source[i] == "*" and i + 1 < n and source[i + 1] == "/":
                    depth -= 1
                    out.append("  ")
                    i += 2
                else:
                    out.append("\n" if source[i] == "\n" else " ")
                    i += 1
            continue
        # Raw string literal: r"...", r#"..."#, br"...", br#"..."#
        if ch in ("r", "b"):
            j = i
            if ch == "b" and j + 1 < n and source[j + 1] == "r":
                j += 1
            if source[j] == "r":
                k = j + 1
                hashes = 0
                while k < n and source[k] == "#":
                    hashes += 1
                    k += 1
                if k < n and source[k] == '"':
                    out.append(" " * (k - i + 1))
                    i = k + 1
                    closer = '"' + ("#" * hashes)
                    while i < n:
                        if source.startswith(closer, i):
                            out.append(" " * len(closer))
                            i += len(closer)
                            break
                        out.append("\n" if source[i] == "\n" else " ")
                        i += 1
                    continue
            if ch == "b" and i + 1 < n and source[i + 1] == '"':
                out.append("  ")
                i += 2
                while i < n:
                    if source[i] == "\\" and i + 1 < n:
                        out.append(" \n" if source[i + 1] == "\n" else "  ")
                        i += 2
                        continue
                    if source[i] == '"':
                        out.append(" ")
                        i += 1
                        break
                    out.append("\n" if source[i] == "\n" else " ")
                    i += 1
                continue
        # Normal string literal
        if ch == '"':
            out.append(" ")
            i += 1
            while i < n:
                if source[i] == "\\" and i + 1 < n:
                    out.append(" \n" if source[i + 1] == "\n" else "  ")
                    i += 2
                    continue
                if source[i] == '"':
                    out.append(" ")
                    i += 1
                    break
                out.append("\n" if source[i] == "\n" else " ")
                i += 1
            continue
        # Char literal (distinguished from lifetime `'a`)
        if ch == "'":
            if i + 2 < n and source[i + 1] == "\\":
                j = i + 2
                while j < n and source[j] not in ("'", "\n"):
                    j += 1
                if j < n and source[j] == "'":
                    out.append(" " * (j - i + 1))
                    i = j + 1
                    continue
            elif i + 2 < n and source[i + 1] not in ("'", "\n") and source[i + 2] == "'":
                out.append("   ")
                i += 3
                continue
        out.append(ch)
        i += 1
    return "".join(out)


def resolve_mod_file(owner: Path, mod_name: str, explicit_path: str | None) -> list[Path]:
    if explicit_path is not None:
        return [(owner.parent / explicit_path).resolve()]
    if owner.name in ("lib.rs", "main.rs", "mod.rs"):
        base = owner.parent
    else:
        base = owner.parent / owner.stem
    return [(base / f"{mod_name}.rs").resolve(), (base / mod_name / "mod.rs").resolve()]


def skip_attr(text: str, pos: int) -> int:
    """Advance past `#![...]` or `#[...]` starting at `pos`."""
    n = len(text)
    i = pos + 1
    if i < n and text[i] == "!":
        i += 1
    while i < n and text[i].isspace():
        i += 1
    if i >= n or text[i] != "[":
        return pos + 1
    depth = 1
    i += 1
    while i < n and depth > 0:
        if text[i] == "[":
            depth += 1
        elif text[i] == "]":
            depth -= 1
        i += 1
    return i


def mask_cfg_test(source: str, file_path: Path) -> tuple[str, set[Path]]:
    """Blank out `#[cfg(test)]` items/modules and collect out-of-line test module files."""
    stripped = strip_comments_and_strings(source)
    buf = list(stripped)
    test_files: set[Path] = set()
    n = len(stripped)
    pos = 0

    while True:
        match = CFG_TEST_RE.search(stripped, pos)
        if not match:
            break
        i = match.end()
        explicit_path: str | None = None
        # Consume any stacked attributes (e.g. `#[path = "..."]`, `#[allow(...)]`).
        while i < n:
            while i < n and stripped[i].isspace():
                i += 1
            if i < n and stripped[i] == "#":
                attr_end = skip_attr(stripped, i)
                raw_attr = source[i:attr_end]
                pm = PATH_ATTR_RE.search(raw_attr)
                if pm:
                    explicit_path = pm.group(1)
                i = attr_end
            else:
                break
        tail = stripped[i:]
        out_mod = OUT_OF_LINE_MOD_RE.match(tail)
        if out_mod:
            for candidate in resolve_mod_file(file_path, out_mod.group(1), explicit_path):
                test_files.add(candidate)
            pos = i + out_mod.end()
            continue
        # Skip item body: either balanced `{ ... }` or up to `;`.
        brace_depth = 0
        seen_brace = False
        j = i
        while j < n:
            c = stripped[j]
            if c == "{":
                brace_depth += 1
                seen_brace = True
            elif c == "}":
                brace_depth -= 1
                if seen_brace and brace_depth == 0:
                    j += 1
                    break
            elif c == ";" and not seen_brace:
                j += 1
                break
            j += 1
        for k in range(i, j):
            if buf[k] != "\n":
                buf[k] = " "
        pos = j

    return "".join(buf), test_files


def is_skipped_path(path: Path, crate_src: Path) -> bool:
    rel_parts = path.relative_to(crate_src).parts[:-1]
    return any(part in SKIP_DIRS for part in rel_parts)


def main() -> int:
    if not CRATES_DIR.is_dir():
        print(f"one listener: crates directory not found: {CRATES_DIR}", file=sys.stderr)
        return 1

    files: list[Path] = []
    for crate_dir in sorted(CRATES_DIR.iterdir()):
        src_dir = crate_dir / "src"
        if not src_dir.is_dir():
            continue
        for rs_file in sorted(src_dir.rglob("*.rs")):
            if not is_skipped_path(rs_file, src_dir):
                files.append(rs_file)

    masked_by_file: dict[Path, str] = {}
    cfg_test_files: set[Path] = set()
    for rs_file in files:
        source = rs_file.read_text(encoding="utf-8")
        masked, extra_test_files = mask_cfg_test(source, rs_file)
        masked_by_file[rs_file.resolve()] = masked
        cfg_test_files.update(extra_test_files)

    offenses: list[str] = []
    checked = 0
    for rs_file in files:
        resolved = rs_file.resolve()
        if resolved in cfg_test_files:
            continue
        checked += 1
        rel = rs_file.relative_to(ROOT).as_posix()
        masked = masked_by_file[resolved]
        for lineno, line in enumerate(masked.splitlines(), start=1):
            for kind, pattern in CHECKS:
                if not pattern.search(line):
                    continue
                if kind == "UnixListener::bind" and rel.startswith(ALLOWED_UNIX_PREFIX):
                    continue
                if kind == "TcpListener::bind" and rel in ALLOWED_TCP_FILES:
                    continue
                offenses.append(f"{rel}:{lineno}: forbidden {kind}")

    if offenses:
        for item in offenses:
            print(item)
        return 1

    print(f"one listener: passed ({checked} files checked)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
