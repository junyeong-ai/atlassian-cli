#!/usr/bin/env bash
# The binary knows what its installation consists of — the skill it deployed,
# the tokens it stored in the OS keychain, its config directory. This script is
# the interactive front door; restating any of that here would be a second
# answer to drift from the first.
set -euo pipefail

BINARY_NAME="atlassian-cli"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

usage() {
    cat <<EOF
Usage: uninstall.sh [options]

Removes the binary, the Claude Code skill it deployed, and every OAuth token
it stored. The global configuration is kept unless --purge-config is given.

Options:
  -y, --yes             Skip the confirmation prompt
      --keep-skill      Leave the deployed skill in place
      --keep-credentials  Leave stored OAuth tokens alone
      --purge-config    Also remove \$HOME/.config/atlassian-cli
  -h, --help            Show this help
EOF
}

assume_yes=false
forwarded=()

while [ "$#" -gt 0 ]; do
    case "$1" in
        -y|--yes) assume_yes=true ;;
        --keep-skill|--keep-credentials|--purge-config) forwarded+=("$1") ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
    esac
    shift
done

binary="$INSTALL_DIR/$BINARY_NAME"
if [ ! -x "$binary" ]; then
    binary="$(command -v "$BINARY_NAME" 2>/dev/null || true)"
fi

if [ -z "$binary" ] || [ ! -x "$binary" ]; then
    echo "$BINARY_NAME is not installed at $INSTALL_DIR/$BINARY_NAME or on PATH." >&2
    echo "If an earlier install left files behind, they are:" >&2
    echo "  \$HOME/.claude/skills/jira-confluence" >&2
    echo "  \$HOME/.config/atlassian-cli" >&2
    exit 1
fi

if ! "$binary" self --help >/dev/null 2>&1; then
    echo "$binary predates 'self uninstall'." >&2
    echo "Install a current release first, then re-run this script:" >&2
    echo "  curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash" >&2
    exit 1
fi

if [ "$assume_yes" != true ]; then
    if [ ! -t 0 ]; then
        echo "Refusing to uninstall without a terminal to confirm at; pass --yes." >&2
        exit 1
    fi
    "$binary" --pretty self status >&2 || true
    read -r -p "Remove this installation? [y/N]: " reply || reply=""
    case "$reply" in
        [yY]) ;;
        *) echo "Cancelled" >&2; exit 1 ;;
    esac
fi

# bash 3.2 (macOS) errors on an empty array under `set -u`.
exec "$binary" self uninstall --yes ${forwarded[@]+"${forwarded[@]}"}
