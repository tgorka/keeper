#!/usr/bin/env python3
"""Keep keeper-core's top-level module ceiling frozen; removals are allowed.

Both declarations (including private/inline modules) and sibling Rust files or
directories count. JSON test vectors are data, not modules. Run the on-disk
acceptance tests with python3 scripts/check-module-guards-test.py.
"""
from pathlib import Path
import re

CORE = Path(__file__).resolve().parents[1] / "src-tauri/crates/keeper-core/src"
FROZEN = set("""
account agents agents_cleanup archive asset_url auth backup badge bots bridge
bridges capture chat config container demo device_name disclosure document drafts
egress error failure file_asset file_properties forges graph hq inbox media notes
notify oauth old_agent_rooms org_account palette panels platform providers
recording registry release_check secret_file send sessions sign_in_link
sign_in_sheet signals size tasks telemetry text_file timeline toml_order
transcription update vault_link verification vm voice voice_reach
""".split())

source = (CORE / "lib.rs").read_text()
# Declarations on a line need not be public, unconditional, or out-of-line.
declared = set(re.findall(r"\bmod\s+(?:r#)?([A-Za-z_][A-Za-z_0-9]*)\s*[;{]", source))
on_disk = {p.name if p.is_dir() else p.stem for p in CORE.iterdir()
           if p.is_dir() or (p.suffix == ".rs" and p.name != "lib.rs")}
new = sorted((declared | on_disk) - FROZEN)
if new:
    raise SystemExit("keeper-core frozen: new top-level module(s): " + ", ".join(new))
print("keeper-core frozen module ceiling passed (61 allowed names)")
