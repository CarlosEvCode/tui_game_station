#!/usr/bin/env bash
set -e

REPO="CarlosEvCode/tui_game_station"
BINARY_NAME="tui-game-station"

echo "======================================================"
echo "          Installing TUI Game Station                 "
echo "======================================================"

# Detect OS
OS="$(uname -s)"
if [ "$OS" != "Linux" ]; then
    echo "Error: TUI Game Station install script currently supports Linux."
    exit 1
fi

# Detect Architecture
ARCH="$(uname -m)"
if [ "$ARCH" != "x86_64" ]; then
    echo "Error: Currently pre-compiled binaries are provided for x86_64 architectures."
    echo "You can compile from source with: cargo build --release"
    exit 1
fi

# Determine target directory
if [ -w "/usr/local/bin" ]; then
    INSTALL_DIR="/usr/local/bin"
else
    INSTALL_DIR="$HOME/.local/bin"
    mkdir -p "$INSTALL_DIR"
fi

echo "--> Fetching latest release info from GitHub..."
LATEST_RELEASE=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null || true)

if [ -z "$LATEST_RELEASE" ]; then
    TAG="v0.1.0"
else
    TAG=$(echo "$LATEST_RELEASE" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
fi

if [ -z "$TAG" ]; then
    TAG="v0.1.0"
fi

TARBALL_NAME="tui_game_station-${TAG}-x86_64-unknown-linux-gnu.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${TARBALL_NAME}"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "--> Downloading ${TARBALL_NAME} (${TAG})..."
if ! curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/${TARBALL_NAME}"; then
    echo "Error: Failed to download release package from ${DOWNLOAD_URL}"
    exit 1
fi

echo "--> Extracting release archive..."
tar -xzf "${TMP_DIR}/${TARBALL_NAME}" -C "$TMP_DIR"

EXTRACTED_DIR="${TMP_DIR}/tui_game_station-${TAG}-x86_64-unknown-linux-gnu"
if [ ! -f "${EXTRACTED_DIR}/${BINARY_NAME}" ]; then
    EXTRACTED_DIR="$TMP_DIR"
fi

echo "--> Installing binary to ${INSTALL_DIR}/${BINARY_NAME}..."
install -m 755 "${EXTRACTED_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"

echo ""
echo "[OK] TUI Game Station (${TAG}) installed successfully!"
echo "Location: ${INSTALL_DIR}/${BINARY_NAME}"
echo ""

if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
    echo "Note: Make sure ${INSTALL_DIR} is in your PATH environment variable."
    echo "You can add it to your ~/.bashrc or ~/.zshrc:"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    echo ""
fi

echo "Run the application with: ${BINARY_NAME}"
