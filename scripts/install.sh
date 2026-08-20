#!/usr/bin/env bash
set -euo pipefail

BINARY_NAME="atlassian-cli"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
REPO="junyeong-ai/atlassian-cli"
VERSION="${ATLASSIAN_CLI_VERSION:-}"
SCRIPT_PATH="${BASH_SOURCE[0]:-$0}"
ORIGINAL_DIR="$(pwd)"

if SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_PATH")" 2>/dev/null && pwd -P)"; then
    :
else
    SCRIPT_DIR="$ORIGINAL_DIR"
fi

PROJECT_ROOT="$ORIGINAL_DIR"
IS_CHECKOUT=false
if [ -f "$SCRIPT_DIR/../Cargo.toml" ] && grep -q '^name = "atlassian-cli"' "$SCRIPT_DIR/../Cargo.toml"; then
    PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"
    IS_CHECKOUT=true
fi

BINARY_TMP_DIR=""
STAGED_BINARY=""

cleanup() {
    [ -n "$BINARY_TMP_DIR" ] && rm -rf "$BINARY_TMP_DIR"
    [ -n "$STAGED_BINARY" ] && rm -f "$STAGED_BINARY"
    return 0
}

trap cleanup EXIT

prompt_choice() {
    local prompt="$1"
    local default="$2"
    local choice=""

    if [ -t 0 ]; then
        read -r -p "$prompt" choice || choice=""
    else
        choice="$default"
    fi

    echo "${choice:-$default}"
}

display_path() {
    local path="$1"

    if [ "$path" = "$HOME" ]; then
        echo "\$HOME"
    elif [[ "$path" == "$HOME/"* ]]; then
        echo "\$HOME/${path#"$HOME"/}"
    else
        echo "$path"
    fi
}

is_valid_release_version() {
    local version="$1"

    [[ "$version" =~ ^[0-9][0-9A-Za-z._+-]*$ ]]
}

path_contains() {
    local needle="$1"
    local entry
    local path_entries

    IFS=: read -r -a path_entries <<< "$PATH"
    for entry in "${path_entries[@]}"; do
        if [ "$entry" = "$needle" ]; then
            return 0
        fi
    done

    return 1
}

detect_platform() {
    local os
    local arch
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)

    case "$os" in
        linux) os="unknown-linux-gnu" ;;
        darwin) os="apple-darwin" ;;
        *) echo "Unsupported OS: $os" >&2; exit 1 ;;
    esac

    case "$arch" in
        x86_64) arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
    esac

    echo "${arch}-${os}"
}

get_latest_version() {
    local latest_url

    latest_url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" 2>/dev/null || true)
    case "$latest_url" in
        */releases/tag/v*)
            latest_url="${latest_url##*/releases/tag/v}"
            echo "${latest_url%%[/?#]*}"
            return 0
            ;;
    esac

    curl -sf "https://api.github.com/repos/$REPO/releases/latest" \
        | grep '"tag_name"' \
        | sed -E 's/.*"v([^"]+)".*/\1/' \
        || echo ""
}

download_binary() {
    local version="$1"
    local target="$2"
    local archive="${BINARY_NAME}-v${version}-${target}.tar.gz"
    local url="https://github.com/$REPO/releases/download/v${version}/${archive}"
    local checksum_url="${url}.sha256"
    local binary_path

    [ -n "$BINARY_TMP_DIR" ] && rm -rf "$BINARY_TMP_DIR"
    BINARY_TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/atlassian-cli-install.XXXXXX")

    echo "Downloading $archive..." >&2
    if ! (cd "$BINARY_TMP_DIR" && curl -fsSLO "$url"); then
        echo "Download failed: $url" >&2
        rm -rf "$BINARY_TMP_DIR"
        BINARY_TMP_DIR=""
        return 1
    fi

    echo "Verifying checksum..." >&2
    if ! (cd "$BINARY_TMP_DIR" && curl -fsSLO "$checksum_url"); then
        echo "Checksum download failed: $checksum_url" >&2
        rm -rf "$BINARY_TMP_DIR"
        BINARY_TMP_DIR=""
        return 1
    fi

    local expected_sum
    local actual_sum
    expected_sum=$(awk 'NR==1 {print $1}' "$BINARY_TMP_DIR/${archive}.sha256")

    if command -v sha256sum >/dev/null; then
        actual_sum=$(cd "$BINARY_TMP_DIR" && sha256sum "$archive" | awk '{print $1}')
    elif command -v shasum >/dev/null; then
        actual_sum=$(cd "$BINARY_TMP_DIR" && shasum -a 256 "$archive" | awk '{print $1}')
    else
        echo "No checksum tool found (need sha256sum or shasum)" >&2
        return 1
    fi

    if [ -z "$expected_sum" ] || [ "$expected_sum" != "$actual_sum" ]; then
        echo "Checksum verification failed for $archive" >&2
        echo "  expected: ${expected_sum:-<none>}" >&2
        echo "  actual:   ${actual_sum:-<none>}" >&2
        return 1
    fi

    verify_attestation "$BINARY_TMP_DIR/$archive"

    echo "Extracting..." >&2
    (cd "$BINARY_TMP_DIR" && tar -xzf "$archive") >&2 || return 1
    binary_path="$BINARY_TMP_DIR/$BINARY_NAME"

    if [ ! -x "$binary_path" ]; then
        echo "Archive did not contain executable $BINARY_NAME" >&2
        return 1
    fi

    echo "$binary_path"
}

cargo_build_release() {
    if ! command -v cargo >/dev/null; then
        echo "cargo is required to build from source" >&2
        return 1
    fi

    # rust-toolchain.toml in the checkout pins the toolchain; a rustup-managed
    # cargo resolves it automatically, so no version is hardcoded here.
    cargo build --release
}

build_from_source() {
    if [ "$IS_CHECKOUT" != true ]; then
        echo "Source build requires running inside an atlassian-cli checkout" >&2
        exit 1
    fi

    echo "Building from source..." >&2
    (cd "$PROJECT_ROOT" && cargo_build_release) >&2
    echo "$PROJECT_ROOT/target/release/$BINARY_NAME"
}

# Defense-in-depth on top of the mandatory sha256 check: the checksum file
# shares its origin with the archive, so it only catches transport corruption,
# while a GitHub attestation proves the artifact was built by this
# repository's release workflow. The three outcomes are reported distinctly —
# tooling absent and unauthenticated are skips, but a verification that RAN
# and rejected the artifact surfaces gh's own diagnostics. Rejection is loud
# yet non-fatal: releases published before attestations existed fail this
# check legitimately, so the installer warns and defers to the operator
# instead of guessing which case it is.
verify_attestation() {
    local archive_path="$1"
    local gh_output

    if ! command -v gh >/dev/null; then
        echo "gh CLI not found; skipping build-provenance verification" >&2
        return 0
    fi

    if ! gh auth status >/dev/null 2>&1; then
        echo "gh CLI is not authenticated; skipping build-provenance verification (run 'gh auth login' to enable it)" >&2
        return 0
    fi

    if gh_output=$(gh attestation verify "$archive_path" --repo "$REPO" 2>&1); then
        echo "Build provenance verified (GitHub attestation)" >&2
        return 0
    fi

    echo "WARNING: build-provenance verification FAILED for $(basename "$archive_path"):" >&2
    printf '%s\n' "$gh_output" >&2
    echo "  No valid attestation from $REPO's release workflow matches this artifact." >&2
    echo "  Releases published before attestations were introduced fail this check legitimately;" >&2
    echo "  for a current release this can indicate a tampered artifact — stop and verify manually." >&2
}

install_binary() {
    local binary_path="$1"

    mkdir -p "$INSTALL_DIR"
    # Stage next to the destination and rename: replacing a running binary
    # in place fails with ETXTBSY on Linux, while rename always succeeds.
    STAGED_BINARY="$INSTALL_DIR/.$BINARY_NAME.tmp.$$"
    cp "$binary_path" "$STAGED_BINARY"
    chmod +x "$STAGED_BINARY"

    if [[ "${OSTYPE:-}" == "darwin"* ]]; then
        codesign --force --deep --sign - "$STAGED_BINARY" 2>/dev/null || true
    fi

    mv -f "$STAGED_BINARY" "$INSTALL_DIR/$BINARY_NAME"
    STAGED_BINARY=""
    echo "Installed to $INSTALL_DIR/$BINARY_NAME" >&2
}

main() {
    echo "Installing Atlassian CLI..." >&2

    local binary_path=""
    local target
    local version="$VERSION"
    local explicit_version=false
    local method
    local display_install_dir
    local command_name

    target=$(detect_platform)

    version="${version#v}"
    if [ "$version" = "latest" ]; then
        version=""
    elif [ -n "$version" ]; then
        explicit_version=true
    fi

    if ! command -v curl >/dev/null; then
        if [ "$IS_CHECKOUT" = true ]; then
            echo "curl not found; building from source" >&2
            version=""
            method="2"
        else
            echo "curl is required to install a prebuilt binary" >&2
            exit 1
        fi
    else
        if [ -z "$version" ]; then
            version=$(get_latest_version)
        fi

        if [ -n "$version" ] && ! is_valid_release_version "$version"; then
            echo "Invalid release version: $version" >&2
            exit 1
        fi

        if [ -n "$version" ]; then
            if [ "$explicit_version" = true ]; then
                echo "Version: v$version" >&2
            else
                echo "Latest release: v$version" >&2
            fi
        else
            echo "Could not determine latest release" >&2
        fi

        if [ "$IS_CHECKOUT" = true ]; then
            echo "" >&2
            echo "Installation method:" >&2
            echo "  [1] Download prebuilt binary" >&2
            echo "  [2] Build from source" >&2
            method=$(prompt_choice "Choose [1-2] (default: 1): " "1")
        else
            method="1"
        fi
    fi

    case "$method" in
        2) binary_path=$(build_from_source) ;;
        1|"")
            if [ -z "$version" ]; then
                echo "Could not resolve a release version. Set ATLASSIAN_CLI_VERSION=x.y.z to install a specific release." >&2
                exit 1
            fi
            binary_path=$(download_binary "$version" "$target")
            ;;
        *) echo "Invalid choice" >&2; exit 1 ;;
    esac

    install_binary "$binary_path"

    echo "" >&2
    display_install_dir=$(display_path "$INSTALL_DIR")
    command_name="$BINARY_NAME"

    if path_contains "$INSTALL_DIR"; then
        echo "$INSTALL_DIR is in PATH" >&2
    else
        command_name="$display_install_dir/$BINARY_NAME"
        echo "$INSTALL_DIR is not in PATH" >&2
        echo "Add this to your shell profile:" >&2
        echo "  export PATH=\"$display_install_dir:\$PATH\"" >&2
    fi

    if [ -x "$INSTALL_DIR/$BINARY_NAME" ]; then
        "$INSTALL_DIR/$BINARY_NAME" --version >&2
    else
        echo "Installed binary is not executable: $INSTALL_DIR/$BINARY_NAME" >&2
        exit 1
    fi

    # The skill is compiled into the binary, so the binary deploys it — there
    # is no version to compare and nothing to fetch separately.
    "$INSTALL_DIR/$BINARY_NAME" self skill install >/dev/null || \
        echo "Could not install the Claude Code skill; run '$command_name self skill install'" >&2

    echo "" >&2
    echo "Installation complete" >&2
    echo "Next steps:" >&2
    echo "  $command_name config init --global" >&2
    echo "  $command_name config show" >&2
    echo "  $command_name jira search \"status = Open\"" >&2
}

if [[ "${BASH_SOURCE[0]:-$0}" == "$0" ]]; then
    main
fi
