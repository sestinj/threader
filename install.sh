#!/bin/sh
set -e

REPO="sestinj/threader"
INSTALL_DIR="$HOME/.local/bin"

# Detect OS
OS="$(uname -s)"
case "$OS" in
  Darwin) os="darwin" ;;
  Linux)  os="linux" ;;
  *)
    echo "Error: Unsupported operating system: $OS" >&2
    exit 1
    ;;
esac

# Detect architecture
ARCH="$(uname -m)"
case "$ARCH" in
  arm64|aarch64) arch="arm64" ;;
  x86_64|amd64)  arch="x86_64" ;;
  *)
    echo "Error: Unsupported architecture: $ARCH" >&2
    exit 1
    ;;
esac

NAME="threader-${os}-${arch}"

# Get latest version
if [ -z "$VERSION" ]; then
  VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed 's/.*"v\(.*\)".*/\1/')"
fi

if [ -z "$VERSION" ]; then
  echo "Error: Could not determine latest version" >&2
  exit 1
fi

URL="https://github.com/${REPO}/releases/download/v${VERSION}/${NAME}.tar.gz"

echo "Installing threader v${VERSION} (${os}/${arch})..."

# Download and extract
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL "$URL" -o "$TMP/${NAME}.tar.gz"
tar xzf "$TMP/${NAME}.tar.gz" -C "$TMP"

# Install
mkdir -p "$INSTALL_DIR"
mv "$TMP/$NAME" "$INSTALL_DIR/threader"
chmod +x "$INSTALL_DIR/threader"

echo "Installed threader to $INSTALL_DIR/threader"

# Check if already in PATH
if echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
  echo "threader is already in your PATH."
else
  echo ""
  echo "Add threader to your PATH by adding this to your shell profile:"
  echo ""
  echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
  echo ""
fi

# Start daemon (also runs init + installs hooks) and log in
"$INSTALL_DIR/threader" start
"$INSTALL_DIR/threader" login

echo ""
echo "\033[47;30m You're all set! Threader is running in the background.                        \033[0m"
echo "\033[47;30m Start a Claude Code session, then watch it appear at https://threader.sh      \033[0m"
echo ""
