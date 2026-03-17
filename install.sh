#!/bin/bash
set -euo pipefail

# cloudcode installer
# Usage: curl -fsSL https://raw.githubusercontent.com/ssreeni1/cloudcode/main/install.sh | bash

REPO="ssreeni1/cloudcode"
INSTALL_DIR="/usr/local/bin"
BINARY="cloudcode"

# Detect platform
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
    darwin) OS="apple-darwin" ;;
    linux)  OS="unknown-linux-gnu" ;;
    *)      echo "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
    x86_64|amd64)   ARCH="x86_64" ;;
    arm64|aarch64)   ARCH="aarch64" ;;
    *)               echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

TARGET="${ARCH}-${OS}"
echo "Detected platform: ${TARGET}"

# Get latest release tag
LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$LATEST" ]; then
    echo "Failed to fetch latest release"
    exit 1
fi
echo "Latest release: ${LATEST}"

# Download
URL="https://github.com/${REPO}/releases/download/${LATEST}/${BINARY}-${TARGET}"
echo "Downloading ${URL}..."

TMP=$(mktemp)
if ! curl -fsSL "$URL" -o "$TMP"; then
    echo "Failed to download binary for ${TARGET}"
    echo "Available at: https://github.com/${REPO}/releases/tag/${LATEST}"
    rm -f "$TMP"
    exit 1
fi

chmod +x "$TMP"

# Install
if [ -w "$INSTALL_DIR" ]; then
    mv "$TMP" "${INSTALL_DIR}/${BINARY}"
else
    echo "Installing to ${INSTALL_DIR} (requires sudo)..."
    sudo mv "$TMP" "${INSTALL_DIR}/${BINARY}"
fi

echo ""
echo "cloudcode installed to ${INSTALL_DIR}/${BINARY}"
echo "Run 'cloudcode' to get started."
