#!/usr/bin/env bash
# contextd installer script
# Usage: curl -fsSL https://raw.githubusercontent.com/sandy-sachin7/contextd/main/scripts/install.sh | bash

set -euo pipefail

REPO="sandy-sachin7/contextd"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}contextd installer${NC}"
echo ""

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
    linux)
        case "$ARCH" in
            x86_64|amd64)  BINARY="contextd-linux-x86_64" ;;
            aarch64|arm64) BINARY="contextd-linux-aarch64" ;;
            *)
                echo -e "${RED}Unsupported architecture: $ARCH${NC}" >&2
                exit 1
                ;;
        esac
        ;;
    darwin)
        case "$ARCH" in
            x86_64)        BINARY="contextd-macos-x86_64" ;;
            arm64|aarch64) BINARY="contextd-macos-aarch64" ;;
            *)
                echo -e "${RED}Unsupported architecture: $ARCH${NC}" >&2
                exit 1
                ;;
        esac
        ;;
    *)
        echo -e "${RED}Unsupported OS: $OS${NC}" >&2
        echo "For Windows, use scripts/install.ps1 or download from: https://github.com/$REPO/releases" >&2
        exit 1
        ;;
esac

echo "Detected: $OS ($ARCH)"
echo "Binary: $BINARY"
echo ""

echo "Fetching latest release..."
LATEST=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' | head -n 1)

if [ -z "$LATEST" ]; then
    echo -e "${RED}Failed to fetch latest release${NC}" >&2
    exit 1
fi

echo "Latest version: $LATEST"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST/$BINARY"
SUMS_URL="https://github.com/$REPO/releases/download/$LATEST/SHA256SUMS.txt"

TMPDIR_INSTALL="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_INSTALL"' EXIT

echo "Downloading $BINARY and SHA256SUMS.txt..."
curl -fsSL "$DOWNLOAD_URL" -o "$TMPDIR_INSTALL/$BINARY"
curl -fsSL "$SUMS_URL" -o "$TMPDIR_INSTALL/SHA256SUMS.txt"

EXPECTED_SHA=$(awk -v name="$BINARY" '$2 == name || $2 == ("*" name) { print $1; exit }' "$TMPDIR_INSTALL/SHA256SUMS.txt")
if [ -z "$EXPECTED_SHA" ]; then
    echo -e "${RED}Checksum entry for $BINARY not found in SHA256SUMS.txt. Aborting.${NC}" >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL_SHA=$(sha256sum "$TMPDIR_INSTALL/$BINARY" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
    ACTUAL_SHA=$(shasum -a 256 "$TMPDIR_INSTALL/$BINARY" | awk '{print $1}')
else
    echo -e "${RED}Neither sha256sum nor shasum found; cannot verify download integrity.${NC}" >&2
    exit 1
fi

if [ "$ACTUAL_SHA" != "$EXPECTED_SHA" ]; then
    echo -e "${RED}SHA-256 verification failed!${NC}" >&2
    echo "Expected: $EXPECTED_SHA" >&2
    echo "Actual:   $ACTUAL_SHA" >&2
    exit 1
fi

echo -e "${GREEN}✓ SHA-256 checksum verified.${NC}"

mkdir -p "$INSTALL_DIR"
chmod +x "$TMPDIR_INSTALL/$BINARY"
mv "$TMPDIR_INSTALL/$BINARY" "$INSTALL_DIR/contextd"

if [ -x "$INSTALL_DIR/contextd" ]; then
    echo ""
    echo -e "${GREEN}✓ contextd $LATEST installed successfully!${NC}"
    echo ""
    echo "Location: $INSTALL_DIR/contextd"
    echo ""

    if ! echo ":$PATH:" | grep -q ":$INSTALL_DIR:"; then
        echo -e "${YELLOW}Note: Add $INSTALL_DIR to your PATH:${NC}"
        echo ""
        echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
        echo ""
        echo "Add this to your ~/.bashrc or ~/.zshrc"
    fi

    echo "Run 'contextd --help' to get started"
else
    echo -e "${RED}Installation failed${NC}" >&2
    exit 1
fi
