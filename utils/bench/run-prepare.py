#!/usr/bin/env python3
"""Thin CLI for utils/bench/prepare.py."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import prepare  # noqa: E402


def main(argv=None) -> int:
    return prepare.main(argv)


if __name__ == "__main__":
    sys.exit(main())
