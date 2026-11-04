#!/usr/bin/env python3
"""Thin CLI for utils/bench/workload.py."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import workload  # noqa: E402


def main(argv=None) -> int:
    return workload.main(argv)


if __name__ == "__main__":
    sys.exit(main())
