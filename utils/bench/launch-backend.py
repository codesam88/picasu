#!/usr/bin/env python3
"""CLI wrapper around `backend_launcher.py`. Configuration is env-driven.

    BUILD_PROFILE=release OUT=<out> PORT=<port> utils/bench/launch-backend.py

Exit codes: 0 completed; 1 config/preflight; 2 startup; 3 reserved;
4 outer timeout. See `backend_launcher.py` for the full environment contract.
"""

import os
import signal
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import backend_launcher  # noqa: E402


def _interrupt(signum, frame):
    raise KeyboardInterrupt


def main() -> int:
    # SIGTERM unwinds through run_backend's finally so the server's process
    # group is stopped instead of orphaned.
    signal.signal(signal.SIGTERM, _interrupt)
    environ = dict(os.environ)
    try:
        cfg = backend_launcher.config_from_env(environ)
    except backend_launcher.ConfigError as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return 1
    try:
        return backend_launcher.run_backend(cfg, environ)
    except KeyboardInterrupt:
        print("interrupted", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
