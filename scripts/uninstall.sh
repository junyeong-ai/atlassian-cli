#!/usr/bin/env bash
set -euo pipefail

BINARY_NAME="atlassian-cli"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
SKILL_NAME="jira-confluence"
USER_SKILL_DIR="$HOME/.claude/skills/$SKILL_NAME"
CONFIG_DIR="$HOME/.config/atlassian-cli"

ASSUME_YES=false
REMOVE_SKILL=""
BACKUP_SKILL=""
REMOVE_CONFIG=""

usage() {
    cat <<EOF
Usage: uninstall.sh [options]

Options:
  -y, --yes             Remove binary, skill, and global configuration without prompts
      --remove-skill    Remove the installed user-level skill
      --keep-skill      Keep the installed user-level skill
      --backup-skill    Back up the user-level skill before removal
      --no-backup-skill Remove the user-level skill without backup
      --remove-config   Remove global configuration
      --keep-config     Keep global configuration
  -h, --help            Show this help
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        -y|--yes)
            ASSUME_YES=true
            ;;
        --remove-skill)
            REMOVE_SKILL=yes
            ;;
        --keep-skill)
            REMOVE_SKILL=no
            ;;
        --backup-skill)
            BACKUP_SKILL=yes
            ;;
        --no-backup-skill)
            BACKUP_SKILL=no
            ;;
        --remove-config)
            REMOVE_CONFIG=yes
            ;;
        --keep-config)
            REMOVE_CONFIG=no
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
    shift
done

if [ "$ASSUME_YES" = true ]; then
    REMOVE_SKILL="${REMOVE_SKILL:-yes}"
    BACKUP_SKILL="${BACKUP_SKILL:-no}"
    REMOVE_CONFIG="${REMOVE_CONFIG:-yes}"
fi

prompt_yes_no() {
    local prompt="$1"
    local default="$2"
    local configured="$3"
    local reply=""

    case "$configured" in
        yes|no)
            [ "$configured" = yes ]
            return
            ;;
    esac

    if [ -t 0 ]; then
        read -r -p "$prompt" reply || reply=""
    else
        reply="$default"
    fi

    reply="${reply:-$default}"
    [[ "$reply" =~ ^[Yy]$ ]]
}

remove_empty_dir() {
    local dir="$1"

    if [ -d "$dir" ] && [ -z "$(find "$dir" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
        rmdir "$dir"
    fi
}

echo "Uninstalling Atlassian CLI..."
echo

if [ -f "$INSTALL_DIR/$BINARY_NAME" ]; then
    rm "$INSTALL_DIR/$BINARY_NAME"
    echo "Removed binary: $INSTALL_DIR/$BINARY_NAME"
else
    echo "Binary not found at $INSTALL_DIR/$BINARY_NAME"
fi

echo
echo "Claude Code Skill"
echo

if [ -d "$USER_SKILL_DIR" ]; then
    echo "User-level skill found at: $USER_SKILL_DIR"
    echo

    if prompt_yes_no "Remove Claude Code skill? [y/N]: " "n" "$REMOVE_SKILL"; then
        if prompt_yes_no "Create backup before removing? [Y/n]: " "y" "$BACKUP_SKILL"; then
            timestamp=$(date +%Y%m%d_%H%M%S)
            backup_dir="$USER_SKILL_DIR.backup_$timestamp"
            cp -R "$USER_SKILL_DIR" "$backup_dir"
            echo "Backup created: $backup_dir"
        fi

        rm -rf "$USER_SKILL_DIR"
        echo "Removed user-level skill"

        remove_empty_dir "$HOME/.claude/skills"
        remove_empty_dir "$HOME/.claude"
    else
        echo "Kept user-level skill"
    fi
else
    echo "No user-level skill found at $USER_SKILL_DIR"
fi

echo
echo "Configuration"
echo

if [ -d "$CONFIG_DIR" ]; then
    echo "Global configuration found at: $CONFIG_DIR"
    echo

    if prompt_yes_no "Remove global configuration? [y/N]: " "n" "$REMOVE_CONFIG"; then
        rm -rf "$CONFIG_DIR"
        echo "Removed global configuration: $CONFIG_DIR"
    else
        echo "Kept global configuration"
    fi
else
    echo "Global configuration not found"
fi

echo
echo "Uninstallation complete"
echo
echo "Remaining items not removed automatically:"
echo "  - Project-level config: ./.atlassian.toml or ./.atlassian/config.toml"
echo "  - Repository checkout files"
echo "  - Environment variables in your shell profile"
echo
echo "To reinstall: curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash"
echo
