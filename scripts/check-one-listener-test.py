#!/usr/bin/env python3
"""On-disk self-tests for scripts/check-one-listener.py."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "check-one-listener.py"


def run_check(tree: Path, expected_ok: bool, expected_text: str | None = None, label: str = "") -> str:
    env = dict(os.environ, KEEPER_LISTENER_ROOT=str(tree))
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if expected_ok:
        assert result.returncode == 0, f"{label} failed unexpectedly:\n{result.stdout}"
    else:
        assert result.returncode != 0, f"{label} passed unexpectedly:\n{result.stdout}"
    if expected_text is not None:
        assert expected_text in result.stdout, (
            f"{label} missing {expected_text!r} in:\n{result.stdout}"
        )
    return result.stdout


def write_file(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="keeper-one-listener-test-") as temporary:
        tree = Path(temporary)
        wire_lib = tree / "src-tauri/crates/keeper-wire/src/lib.rs"
        loopback = tree / "src-tauri/crates/keeper-core/src/org_account/loopback.rs"
        other_src = tree / "src-tauri/crates/keeper-other/src/lib.rs"
        other_tests = tree / "src-tauri/crates/keeper-other/src/tests/helper.rs"

        write_file(
            wire_lib,
            "pub fn serve(p: &str) { let _ = tokio::net::UnixListener::bind(p); }\n",
        )
        write_file(
            loopback,
            "pub fn listen() { let _ = std::net::TcpListener::bind(\"127.0.0.1:0\"); }\n",
        )

        # 1. Allowed UnixListener::bind in keeper-wire and TcpListener::bind in loopback.rs -> GREEN.
        run_check(tree, True, "one listener: passed", "baseline GREEN")

        # 2. Second UnixListener::bind in another crate's src -> RED naming the file.
        write_file(
            other_src,
            "pub fn rogue(p: &str) { let _ = std::os::unix::net::UnixListener::bind(p); }\n",
        )
        run_check(
            tree,
            False,
            "src-tauri/crates/keeper-other/src/lib.rs:1",
            "second UnixListener::bind RED",
        )

        # 3. Same bind inside #[cfg(test)] mod tests { ... } -> GREEN.
        write_file(
            other_src,
            "#[cfg(test)]\nmod tests {\n    fn t(p: &str) { let _ = std::os::unix::net::UnixListener::bind(p); }\n}\n",
        )
        run_check(tree, True, "one listener: passed", "cfg(test) UnixListener::bind GREEN")

        # 4. TcpListener::bind outside loopback.rs -> RED.
        write_file(
            other_src,
            "pub fn rogue_tcp() { let _ = std::net::TcpListener::bind(\"127.0.0.1:0\"); }\n",
        )
        run_check(
            tree,
            False,
            "src-tauri/crates/keeper-other/src/lib.rs:1",
            "outside loopback TcpListener::bind RED",
        )

        # 5. Bind inside a tests/ dir -> GREEN.
        other_src.unlink()
        write_file(
            other_tests,
            "pub fn test_tcp() { let _ = std::net::TcpListener::bind(\"127.0.0.1:0\"); }\n",
        )
        run_check(tree, True, "one listener: passed", "tests/ dir TcpListener::bind GREEN")

        # 6. UdpSocket::bind and abstract namespace -> RED.
        write_file(
            other_src,
            "pub fn udp() { let _ = std::net::UdpSocket::bind(\"127.0.0.1:0\"); }\n",
        )
        run_check(tree, False, "UdpSocket::bind", "UdpSocket::bind RED")

        write_file(
            wire_lib,
            "pub fn abs() { let _ = SocketAddr::from_abstract_name(b\"keeper\"); }\n",
        )
        other_src.unlink()
        run_check(tree, False, "abstract-namespace", "from_abstract_name RED")

    print("one-listener self-tests passed")


if __name__ == "__main__":
    main()
