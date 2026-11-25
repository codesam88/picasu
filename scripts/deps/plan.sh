#!/usr/bin/env bash
# scripts/deps/plan.sh — ensure/check plan
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

install_from_cargo() {
    if ! command -v cargo >/dev/null 2>&1; then
        echo "cargo not found — cannot install plan" >&2
        return 1
    fi
    echo "→ installing plan $PLAN_VERSION from source"
    cargo install --locked --git https://github.com/tedsamhain/tablethat --tag "v$PLAN_VERSION"
}

ensure() {
    local want="$PLAN_VERSION"
    if command -v plan >/dev/null 2>&1 && [[ "$(plan --version 2>/dev/null || true)" == "plan $want" ]]; then
        echo "✓ plan $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ plan: want $want, got $(plan --version 2>/dev/null || echo '<missing>')"
        return 1
    fi
    local target sha
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) target="x86_64-unknown-linux-musl"; sha="$PLAN_SHA256_LINUX_X86_64" ;;
        Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-musl"; sha="$PLAN_SHA256_LINUX_AARCH64" ;;
        Darwin-x86_64) target="x86_64-apple-darwin"; sha="$PLAN_SHA256_MACOS_X86_64" ;;
        Darwin-arm64) target="aarch64-apple-darwin"; sha="$PLAN_SHA256_MACOS_AARCH64" ;;
        *)
            echo "no prebuilt plan for $(uname -s)-$(uname -m); falling back to cargo install" >&2
            install_from_cargo
            return $?
            ;;
    esac
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp:-}"' EXIT
    local url="${PLAN_URL_TEMPLATE//\{version\}/$want}"
    url="${url//\{target\}/$target}"
    local file="$tmp/plan.tar.gz"
    echo "→ downloading plan $want ($target)"
    curl -fsSL --retry 3 --retry-all-errors -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$sha  $file" | sha256sum -c -
    else
        echo "$sha  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    tar -xzf "$file" -C "$tmp"
    local bin
    bin="$(find "$tmp" -name plan -type f | head -n 1)"
    if [[ -z "${bin:-}" ]]; then
        echo "plan binary not found in release tarball" >&2
        return 1
    fi
    install -m755 "$bin" "$HOME/.local/bin/plan"
    rm -rf "$tmp"
    trap - EXIT
    echo "✓ plan $want installed — $HOME/.local/bin/plan"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi
