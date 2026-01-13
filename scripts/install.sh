#!/bin/bash
# contextd installer script
# Usage: curl -sSL https://raw.githubusercontent.com/sandy-sachin7/contextd/main/scripts/install.sh | sh

set -e

REPO="sandy-sachin7/contextd"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}contextd installer${NC}"
echo ""

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
    linux)
        case "$ARCH" in
            x86_64)
                BINARY="contextd-linux-x86_64"
                ;;
            aarch64|arm64)
                BINARY="contextd-linux-aarch64"
                ;;
            *)
                echo -e "${RED}Unsupported architecture: $ARCH${NC}"
                exit 1
                ;;
        esac
        ;;
    darwin)
        case "$ARCH" in
            x86_64)
                BINARY="contextd-macos-x86_64"
                ;;
            arm64)
                BINARY="contextd-macos-aarch64"
                ;;
            *)
                echo -e "${RED}Unsupported architecture: $ARCH${NC}"
                exit 1
                ;;
        esac
        ;;
    *)
        echo -e "${RED}Unsupported OS: $OS${NC}"
        echo "For Windows, download from: https://github.com/$REPO/releases"
        exit 1
        ;;
esac

echo "Detected: $OS ($ARCH)"
echo "Binary: $BINARY"
echo ""

# Get latest release
echo "Fetching latest release..."
LATEST=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$LATEST" ]; then
    echo -e "${RED}Failed to fetch latest release${NC}"
    exit 1
fi

echo "Latest version: $LATEST"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST/$BINARY"

# Create install directory
mkdir -p "$INSTALL_DIR"

# Download binary
echo "Downloading $BINARY..."
curl -sL "$DOWNLOAD_URL" -o "$INSTALL_DIR/contextd"

# Make executable
chmod +x "$INSTALL_DIR/contextd"

# Verify installation
if [ -x "$INSTALL_DIR/contextd" ]; then
    echo ""
    echo -e "${GREEN}✓ contextd installed successfully!${NC}"
    echo ""
    echo "Location: $INSTALL_DIR/contextd"
    echo ""

    # Check if in PATH
    if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
        echo -e "${YELLOW}Note: Add $INSTALL_DIR to your PATH:${NC}"
        echo ""
        echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
        echo ""
        echo "Add this to your ~/.bashrc or ~/.zshrc"
    fi

    echo "Run 'contextd --help' to get started"
else
    echo -e "${RED}Installation failed${NC}"
    exit 1
fi
