#!/usr/bin/env bash
set -e

BINARY_NAME="atlassian"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

echo "🗑️  Uninstalling Atlassian CLI..."
echo

if [ -f "$INSTALL_DIR/$BINARY_NAME" ]; then
    rm "$INSTALL_DIR/$BINARY_NAME"
    echo "✅ Removed $INSTALL_DIR/$BINARY_NAME"
else
    echo "⚠️  Binary not found at $INSTALL_DIR/$BINARY_NAME"
fi

# Remove global config (optional)
echo
read -p "Remove global configuration? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    # Check both Linux and macOS locations
    REMOVED=false

    # Linux/XDG location
    if [ -d "$HOME/.config/atlassian-cli" ]; then
        rm -rf "$HOME/.config/atlassian-cli"
        echo "✅ Removed ~/.config/atlassian-cli"
        REMOVED=true
    fi

    # macOS location
    if [ -d "$HOME/Library/Application Support/atlassian" ]; then
        rm -rf "$HOME/Library/Application Support/atlassian"
        echo "✅ Removed ~/Library/Application Support/atlassian"
        REMOVED=true
    fi

    if [ "$REMOVED" = false ]; then
        echo "⚠️  Global config not found"
    fi
fi

echo
echo "✅ Uninstallation complete!"
echo
echo "Note: Project-level .atlassian.toml files are NOT removed automatically."
echo "If you have project configs, remove them manually if needed."
echo
