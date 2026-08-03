#!/usr/bin/env bash
set -e

REPO="CarlosEvCode/tui_game_station"
BINARY_NAME="tui-game-station"

cat << "EOF"
  _____   ___  ___  ___ _____   _____ _____ ___ _____ _____ _____ _   _ 
|  __ \ / _ \ |  \/  ||  ___| /  ___|_   _/ _ \_   _|_   _|  _  | \ | |
| |  \// /_\ \| .  . || |__   \ `--.  | |/ /_\ \| |   | | | | | |  \| |
| | __ |  _  || |\/| ||  __|   `--. \ | ||  _  || |   | | | | | | . ` |
| |_\ \| | | || |  | || |___  /\__/ / | || | | || |  _| |_\ \_/ / |\  |
 \____/\_| |_/\_|  |_/\____/  \____/  \_/\_| |_/\_/  \___/ \___/\_| \_/
EOF
echo ""

# Parse arguments
for arg in "$@"; do
    if [ "$arg" = "--uninstall" ]; then
        echo "--> Uninstalling TUI Game Station..."
        rm -f "/usr/local/bin/${BINARY_NAME}" "$HOME/.local/bin/${BINARY_NAME}" 2>/dev/null || true
        for subarg in "$@"; do
            if [ "$subarg" = "--purge" ] || [ "$subarg" = "-p" ]; then
                echo "--> Purging data at $HOME/.config/tui_game_station..."
                rm -rf "$HOME/.config/tui_game_station"
            fi
        done
        echo "[OK] TUI Game Station uninstalled successfully."
        exit 0
    fi
done

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

# ── Check if INSTALL_DIR is in PATH (POSIX-compatible) ──────────────────────
case ":$PATH:" in
    *":${INSTALL_DIR}:"*)
        # Already in PATH — nothing to do
        echo "Run the application with: ${BINARY_NAME}"
        ;;
    *)
        # Not in PATH — detect which shells the user has and inject accordingly
        ADDED_TO=""
        SOURCED_RC=""

        # ── Fish shell ────────────────────────────────────────────────────────
        # Fish uses a completely different syntax: fish_add_path or set -gx
        if command -v fish >/dev/null 2>&1; then
            FISH_CONFIG="$HOME/.config/fish/config.fish"
            mkdir -p "$(dirname "$FISH_CONFIG")"
            if [ -f "$FISH_CONFIG" ]; then
                FISH_CHECK="$HOME/.local/bin"
            else
                FISH_CHECK=""
            fi
            if [ -f "$FISH_CONFIG" ] && grep -q 'local/bin' "$FISH_CONFIG" 2>/dev/null; then
                : # already configured
            else
                printf '\n# Added by tui-game-station installer\nfish_add_path "$HOME/.local/bin"\n' >> "$FISH_CONFIG"
                ADDED_TO="$ADDED_TO $FISH_CONFIG"
            fi
        fi

        # ── Bash / Zsh / POSIX sh ─────────────────────────────────────────────
        for RC in "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.zshrc" "$HOME/.profile"; do
            if [ -f "$RC" ]; then
                if ! grep -q 'local/bin' "$RC" 2>/dev/null; then
                    printf '\n# Added by tui-game-station installer\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$RC"
                    ADDED_TO="$ADDED_TO $RC"
                fi
                # Source the first available POSIX rc to activate PATH in this session
                if [ -z "$SOURCED_RC" ]; then
                    # shellcheck disable=SC1090
                    . "$RC" 2>/dev/null && SOURCED_RC="$RC"
                fi
            fi
        done

        if [ -n "$ADDED_TO" ]; then
            echo "--> PATH updated automatically in:${ADDED_TO}"
        fi

        if [ -n "$SOURCED_RC" ]; then
            echo "--> Shell environment reloaded from ${SOURCED_RC}."
            echo ""
            echo "Run the application with: ${BINARY_NAME}"
        else
            echo ""
            echo "---------------------------------------------------------------------"
            echo "  NOTE: Could not auto-reload your shell environment."
            echo "  Please run the following to start using ${BINARY_NAME}:"
            echo ""
            echo "    source ~/.bashrc && ${BINARY_NAME}   # bash"
            echo "    source ~/.zshrc  && ${BINARY_NAME}   # zsh"
            echo "    exec fish        && ${BINARY_NAME}   # fish"
            echo ""
            echo "  Or simply open a new terminal and run: ${BINARY_NAME}"
            echo "---------------------------------------------------------------------"
        fi
        ;;
esac
