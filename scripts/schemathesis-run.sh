#!/usr/bin/env bash
# Run pinned Schemathesis against a target backend (thin launcher).
# Usage: schemathesis-run.sh <version> <schema> <base-url> [schemathesis args...]
#
# The target must already be running and seeded; harness pieces (config
# file, hooks module, phase/check selection) pass through as extra args.
# See docs/briefs/schemathesis.md for the tool model.
set -euo pipefail

version="${1:?usage: schemathesis-run.sh <version> <schema> <base-url> [args...]}"
schema="${2:?missing schema}"
base_url="${3:?missing base-url}"
shift 3

exec uvx schemathesis@"$version" run "$schema" --url "$base_url" "$@"
