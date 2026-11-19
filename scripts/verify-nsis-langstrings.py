#!/usr/bin/env python3
"""Ensure Shale English.nsh defines every LangString referenced by the Tauri NSIS template."""
from __future__ import annotations

import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ENGLISH = ROOT / "apps/desktop/src-tauri/windows/nsis/English.nsh"
TAURI_TAG = "tauri-v2.12.6"
INSTALLER_URL = (
    f"https://raw.githubusercontent.com/tauri-apps/tauri/{TAURI_TAG}/"
    "crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi"
)


def load_installer() -> str:
    return urllib.request.urlopen(INSTALLER_URL, timeout=60).read().decode("utf-8")


def langs_in_english(path: Path) -> set[str]:
    text = path.read_text(encoding="utf-8")
    return set(re.findall(r"LangString\s+(\w+)", text))


def refs_in_installer(nsi: str) -> set[str]:
    refs = set(re.findall(r"\$\(([A-Za-z][A-Za-z0-9_]*)\)", nsi))
    # Present only on installer languages; not required in custom English.nsh
    refs -= {"LANGUAGE", "choowHowToInstall"}  # defined in installer.nsi before custom lang include
    return refs


def main() -> int:
    if not ENGLISH.is_file():
        print(f"missing {ENGLISH}", file=sys.stderr)
        return 1
    defined = langs_in_english(ENGLISH)
    installer = load_installer()
    needed = refs_in_installer(installer)
    missing = sorted(needed - defined)
    if missing:
        print("Missing LangString definitions:", ", ".join(missing), file=sys.stderr)
        return 1
    print(f"OK: {len(defined)} LangStrings cover {len(needed)} template refs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())