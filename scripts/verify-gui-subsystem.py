#!/usr/bin/env python3
"""Fail if a Windows PE executable is not IMAGE_SUBSYSTEM_WINDOWS_GUI (2)."""
from __future__ import annotations

import struct
import sys

IMAGE_SUBSYSTEM_WINDOWS_GUI = 2
IMAGE_SUBSYSTEM_WINDOWS_CUI = 3


def pe_subsystem(path: str) -> int:
    with open(path, "rb") as handle:
        handle.seek(0x3C)
        pe_offset = struct.unpack("<I", handle.read(4))[0]
        handle.seek(pe_offset + 0x5C)
        return struct.unpack("<H", handle.read(2))[0]


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(f"usage: {argv[0]} <path-to-exe>", file=sys.stderr)
        return 2
    path = argv[1]
    subsystem = pe_subsystem(path)
    print(f"{path}: PE subsystem = {subsystem}")
    if subsystem == IMAGE_SUBSYSTEM_WINDOWS_GUI:
        return 0
    if subsystem == IMAGE_SUBSYSTEM_WINDOWS_CUI:
        print("expected GUI subsystem (2), got console (3)", file=sys.stderr)
        return 1
    print(f"unexpected subsystem {subsystem}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))