#!/usr/bin/env bash
# scripts/deps/shellcheck.sh — ensure/check shellcheck
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

ensure() {
    local want="$SHELLCHECK_VERSION"
    if command -v shellcheck >/dev/null 2>&1 && shellcheck --version 2>/dev/null | grep -qx "version: $want"; then
        echo "✓ shellcheck $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        local got
        got="$(shellcheck --version 2>/dev/null | sed -n 's/^version: //p' || true)"
        echo "✗ shellcheck: want $want, got ${got:-<missing>}"
        return 1
    fi
    local target sha
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) target="linux.x86_64"; sha="$SHELLCHECK_SHA256_LINUX_X86_64" ;;
        Linux-aarch64|Linux-arm64) target="linux.aarch64"; sha="$SHELLCHECK_SHA256_LINUX_AARCH64" ;;
        Darwin-x86_64) target="darwin.x86_64"; sha="$SHELLCHECK_SHA256_MACOS_X86_64" ;;
        Darwin-arm64) target="darwin.aarch64"; sha="$SHELLCHECK_SHA256_MACOS_AARCH64" ;;
        *) echo "no prebuilt shellcheck for $(uname -s)-$(uname -m)" >&2; return 1 ;;
    esac
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp:-}"' EXIT
    local url="${SHELLCHECK_URL_TEMPLATE//\{version\}/$want}"
    url="${url//\{target\}/$target}"
    local file="$tmp/shellcheck.tar.gz"
    echo "→ downloading shellcheck $want ($target)"
    curl -fsSL --retry 3 --retry-all-errors -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$sha  $file" | sha256sum -c -
    else
        echo "$sha  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    tar -xzf "$file" -C "$HOME/.local/bin" --strip-components=1 "shellcheck-v$want/shellcheck"
    chmod +x "$HOME/.local/bin/shellcheck"
    rm -rf "$tmp"
    trap - EXIT
    echo "✓ shellcheck $want installed — $HOME/.local/bin/shellcheck"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi