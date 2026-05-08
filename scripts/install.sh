#!/usr/bin/env bash
set -euo pipefail

BINARY_NAME="atlassian-cli"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
REPO="junyeong-ai/atlassian-cli"
SKILL_NAME="jira-confluence"
USER_SKILL_DIR="$HOME/.claude/skills/$SKILL_NAME"
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

PROJECT_SKILL_DIR="$PROJECT_ROOT/.claude/skills/$SKILL_NAME"
SKILL_SOURCE_DIR=""
SKILL_TMP_DIR=""
BINARY_TMP_DIR=""

cleanup() {
    [ -n "$SKILL_TMP_DIR" ] && rm -rf "$SKILL_TMP_DIR"
    [ -n "$BINARY_TMP_DIR" ] && rm -rf "$BINARY_TMP_DIR"
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
    local archive
    local url
    local checksum_url
    local binary_path

    if [ -n "$version" ]; then
        archive="${BINARY_NAME}-v${version}-${target}.tar.gz"
        url="https://github.com/$REPO/releases/download/v${version}/${archive}"
    else
        archive="${BINARY_NAME}-${target}.tar.gz"
        url="https://github.com/$REPO/releases/latest/download/${archive}"
    fi
    checksum_url="${url}.sha256"

    [ -n "$BINARY_TMP_DIR" ] && rm -rf "$BINARY_TMP_DIR"
    BINARY_TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/atlassian-cli-install.XXXXXX")

    echo "Downloading $archive..." >&2
    if ! (cd "$BINARY_TMP_DIR" && curl -fsSLO "$url"); then
        echo "Download failed" >&2
        rm -rf "$BINARY_TMP_DIR"
        BINARY_TMP_DIR=""
        return 2
    fi

    echo "Verifying checksum..." >&2
    if ! (cd "$BINARY_TMP_DIR" && curl -fsSLO "$checksum_url"); then
        echo "Checksum download failed" >&2
        rm -rf "$BINARY_TMP_DIR"
        BINARY_TMP_DIR=""
        return 1
    fi

    if command -v sha256sum >/dev/null; then
        (cd "$BINARY_TMP_DIR" && sha256sum -c "${archive}.sha256") >&2 || return 1
    elif command -v shasum >/dev/null; then
        (cd "$BINARY_TMP_DIR" && shasum -a 256 -c "${archive}.sha256") >&2 || return 1
    else
        echo "No checksum tool found" >&2
        return 1
    fi

    echo "Extracting..." >&2
    (cd "$BINARY_TMP_DIR" && tar -xzf "$archive") >&2 || return 1
    binary_path="$BINARY_TMP_DIR/$BINARY_NAME"

    if [ ! -x "$binary_path" ]; then
        echo "Archive did not contain executable $BINARY_NAME" >&2
        return 1
    fi

    echo "$binary_path"
}

resolve_prebuilt_binary() {
    local version="$1"
    local target="$2"
    local allow_latest_fallback="$3"
    local binary_path
    local status

    if [ -n "$version" ]; then
        set +e
        binary_path=$(download_binary "$version" "$target")
        status=$?
        set -e

        if [ "$status" -eq 0 ]; then
            echo "$binary_path"
            return 0
        fi

        if [ "$status" -eq 2 ] && [ "$allow_latest_fallback" = true ]; then
            echo "Versioned asset unavailable; trying latest asset name" >&2
            download_binary "" "$target"
            return $?
        fi

        return "$status"
    fi

    download_binary "" "$target"
}

cargo_build_release() {
    if ! command -v cargo >/dev/null; then
        echo "cargo is required to build from source" >&2
        return 1
    fi

    if cargo +1.95.0 --version >/dev/null 2>&1; then
        cargo +1.95.0 build --release
    else
        cargo build --release
    fi
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

install_binary() {
    local binary_path="$1"

    mkdir -p "$INSTALL_DIR"
    cp "$binary_path" "$INSTALL_DIR/$BINARY_NAME"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"

    if [[ "${OSTYPE:-}" == "darwin"* ]]; then
        codesign --force --deep --sign - "$INSTALL_DIR/$BINARY_NAME" 2>/dev/null || true
    fi

    echo "Installed to $INSTALL_DIR/$BINARY_NAME" >&2
}

get_skill_version() {
    local skill_md="$1"
    [ -f "$skill_md" ] && grep "^version:" "$skill_md" 2>/dev/null | sed 's/version: *//' || echo "unknown"
}

check_skill_exists() {
    [ -d "$USER_SKILL_DIR" ] && [ -f "$USER_SKILL_DIR/SKILL.md" ]
}

compare_versions() {
    local ver1="$1"
    local ver2="$2"
    local i
    local a
    local b
    local parts1
    local parts2

    if [ "$ver1" = "$ver2" ]; then
        echo "equal"
        return 0
    fi

    if [ "$ver1" = "unknown" ] || [ "$ver2" = "unknown" ]; then
        echo "unknown"
        return 0
    fi

    if ! [[ "$ver1" =~ ^[0-9]+(\.[0-9]+)*$ && "$ver2" =~ ^[0-9]+(\.[0-9]+)*$ ]]; then
        echo "unknown"
        return 0
    fi

    IFS=. read -r -a parts1 <<< "$ver1"
    IFS=. read -r -a parts2 <<< "$ver2"

    for i in 0 1 2; do
        a="${parts1[$i]:-0}"
        b="${parts2[$i]:-0}"
        if ((10#$a < 10#$b)); then
            echo "older"
            return 0
        fi
        if ((10#$a > 10#$b)); then
            echo "newer"
            return 0
        fi
    done

    echo "equal"
    return 0
}

backup_skill() {
    local timestamp
    local backup_dir
    timestamp=$(date +%Y%m%d_%H%M%S)
    backup_dir="$USER_SKILL_DIR.backup_$timestamp"

    echo "Creating skill backup: $backup_dir" >&2
    cp -r "$USER_SKILL_DIR" "$backup_dir"
}

install_skill() {
    echo "Installing skill to $USER_SKILL_DIR" >&2
    mkdir -p "$(dirname "$USER_SKILL_DIR")"
    rm -rf "$USER_SKILL_DIR"
    cp -r "$SKILL_SOURCE_DIR" "$USER_SKILL_DIR"
    echo "Skill installed" >&2
}

prepare_skill_source() {
    local ref="$1"
    local skill_url
    local fallback_url

    if [ "$IS_CHECKOUT" = true ] && [ -f "$PROJECT_SKILL_DIR/SKILL.md" ]; then
        SKILL_SOURCE_DIR="$PROJECT_SKILL_DIR"
        return 0
    fi

    SKILL_TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/atlassian-cli-skill.XXXXXX")
    SKILL_SOURCE_DIR="$SKILL_TMP_DIR/$SKILL_NAME"
    mkdir -p "$SKILL_SOURCE_DIR"

    skill_url="https://raw.githubusercontent.com/$REPO/$ref/.claude/skills/$SKILL_NAME/SKILL.md"
    if curl -fsSL "$skill_url" -o "$SKILL_SOURCE_DIR/SKILL.md"; then
        return 0
    fi

    if [ "$ref" != "main" ]; then
        fallback_url="https://raw.githubusercontent.com/$REPO/main/.claude/skills/$SKILL_NAME/SKILL.md"
        echo "Skill not found at $ref; trying main" >&2
        if curl -fsSL "$fallback_url" -o "$SKILL_SOURCE_DIR/SKILL.md"; then
            return 0
        fi
    fi

    rm -rf "$SKILL_TMP_DIR"
    SKILL_TMP_DIR=""
    SKILL_SOURCE_DIR=""
    return 1
}

prompt_skill_installation() {
    local ref="$1"
    local project_version
    local existing_version
    local comparison
    local choice

    if ! prepare_skill_source "$ref"; then
        echo "Could not fetch $SKILL_NAME skill from $ref; skipping skill installation" >&2
        return 0
    fi

    project_version=$(get_skill_version "$SKILL_SOURCE_DIR/SKILL.md")

    echo "" >&2
    echo "Claude Code skill: $SKILL_NAME (v$project_version)" >&2

    if check_skill_exists; then
        existing_version=$(get_skill_version "$USER_SKILL_DIR/SKILL.md")
        comparison=$(compare_versions "$existing_version" "$project_version")
        echo "Current skill: v$existing_version" >&2

        case "$comparison" in
            equal)
                choice=$(prompt_choice "Reinstall skill? [y/N]: " "n")
                if [[ "$choice" =~ ^[yY]$ ]]; then
                    backup_skill
                    install_skill
                else
                    echo "Keeping existing skill" >&2
                fi
                ;;
            older)
                choice=$(prompt_choice "Update skill? [Y/n]: " "y")
                if [[ "$choice" =~ ^[nN]$ ]]; then
                    echo "Keeping existing skill" >&2
                else
                    backup_skill
                    install_skill
                fi
                ;;
            newer)
                choice=$(prompt_choice "Installed skill is newer. Replace it? [y/N]: " "n")
                if [[ "$choice" =~ ^[yY]$ ]]; then
                    backup_skill
                    install_skill
                else
                    echo "Keeping existing skill" >&2
                fi
                ;;
            *)
                choice=$(prompt_choice "Install fetched skill over existing skill? [y/N]: " "n")
                if [[ "$choice" =~ ^[yY]$ ]]; then
                    backup_skill
                    install_skill
                else
                    echo "Keeping existing skill" >&2
                fi
                ;;
        esac
    else
        choice=$(prompt_choice "Install Claude Code skill to ~/.claude/skills? [Y/n]: " "y")
        if [[ "$choice" =~ ^[nN]$ ]]; then
            echo "Skipped skill installation" >&2
        else
            install_skill
        fi
    fi
}

main() {
    echo "Installing Atlassian CLI..." >&2

    local binary_path=""
    local target
    local version="$VERSION"
    local explicit_version=false
    local allow_latest_fallback=true
    local ref
    local method
    local display_install_dir
    local command_name

    target=$(detect_platform)

    version="${version#v}"
    if [ "$version" = "latest" ]; then
        version=""
    elif [ -n "$version" ]; then
        explicit_version=true
        allow_latest_fallback=false
    fi

    if ! command -v curl >/dev/null; then
        if [ "$IS_CHECKOUT" = true ]; then
            echo "curl not found; building from source" >&2
            version=""
            ref="main"
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
            ref="v$version"
            if [ "$explicit_version" = true ]; then
                echo "Version: v$version" >&2
            else
                echo "Latest release: v$version" >&2
            fi
        else
            ref="main"
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
        1|"") binary_path=$(resolve_prebuilt_binary "$version" "$target" "$allow_latest_fallback") ;;
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

    prompt_skill_installation "$ref"

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
