#!/usr/bin/env bash
set -e

BINARY_NAME="atlassian"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

echo "🚀 Installing Atlassian CLI..."
echo

# Build release binary
echo "📦 Building release binary..."
cargo build --release

# Create install directory if it doesn't exist
mkdir -p "$INSTALL_DIR"

# Copy binary
echo "📋 Installing to $INSTALL_DIR/$BINARY_NAME"
cp "target/release/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"

# macOS: Ad-hoc sign the binary to prevent "Killed: 9" errors
if [[ "$OSTYPE" == "darwin"* ]]; then
    echo "🔏 Signing binary (macOS)..."
    codesign --force --deep --sign - "$INSTALL_DIR/$BINARY_NAME" 2>/dev/null || true
fi

echo
echo "✅ Installation complete!"
echo
echo "Binary installed to: $INSTALL_DIR/$BINARY_NAME"
echo

# Check if in PATH
if echo "$PATH" | grep -q "$INSTALL_DIR"; then
    echo "✅ $INSTALL_DIR is in your PATH"
    echo
    echo "You can now run: $BINARY_NAME --help"
else
    echo "⚠️  $INSTALL_DIR is not in your PATH"
    echo
    echo "Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    echo
    echo "Then reload your shell:"
    echo "  source ~/.zshrc  # or ~/.bashrc"
fi
echo

# Check version
if command -v "$BINARY_NAME" &> /dev/null; then
    echo "Installed version:"
    "$BINARY_NAME" --version
    echo
fi

# Show config initialization hint
echo "💡 Quick Start:"
echo "  1. Initialize global config:"
echo "     $BINARY_NAME config init --global"
echo
echo "  2. Edit config file:"
echo "     $BINARY_NAME config edit --global"
echo
echo "  3. Add your Atlassian credentials:"
echo "     - domain: company.atlassian.net"
echo "     - email: your@email.com"
echo "     - API token: (from https://id.atlassian.com/manage-profile/security/api-tokens)"
echo
