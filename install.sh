#!/usr/bin/env bash
set -e

# Twominal Installer Script
# Usage: curl -fsSL https://raw.githubusercontent.com/two-net/twominal/main/install.sh | bash

REPO="two-net/twominal"
APP_NAME="Twominal"

# Colors
BOLD="$(tput bold 2>/dev/null || echo '')"
GREEN="$(tput setaf 2 2>/dev/null || echo '')"
CYAN="$(tput setaf 6 2>/dev/null || echo '')"
YELLOW="$(tput setaf 3 2>/dev/null || echo '')"
RED="$(tput setaf 1 2>/dev/null || echo '')"
RESET="$(tput sgr0 2>/dev/null || echo '')"

echo "${CYAN}${BOLD}"
cat << 'EOF'
  _____                         _             _ 
 |_   _|_      _____  _ __ ___ (_)_ __   __ _| |
   | | \ \ /\ / / _ \| '_ ` _ \| | '_ \ / _` | |
   | |  \ V  V / (_) | | | | | | | | | | (_| | |
   |_|   \_/\_/ \___/|_| |_| |_|_|_| |_|\__,_|_|
EOF
echo "${RESET}"
echo "${BOLD}Installing ${APP_NAME}...${RESET}\n"

# 1. Detect OS and Architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "$OS" != "Darwin" ]; then
  echo "${RED}❌ Currently, the automated installer script supports macOS.${RESET}"
  echo "For Linux/Windows, please check the releases at https://github.com/${REPO}/releases"
  exit 1
fi

case "$ARCH" in
  arm64|aarch64)
    ARCH_KEY="aarch64"
    ;;
  x86_64|amd64)
    ARCH_KEY="x86_64"
    ;;
  *)
    echo "${RED}❌ Unsupported architecture: ${ARCH}${RESET}"
    exit 1
    ;;
esac

echo " detected ${GREEN}macOS ($ARCH_KEY)${RESET}"

# 2. Fetch Latest Release Information
echo "🔍 Fetching latest release info from GitHub..."
RELEASE_JSON=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null || echo "")

if [ -z "$RELEASE_JSON" ] || echo "$RELEASE_JSON" | grep -q "API rate limit exceeded"; then
  # Fallback to direct download URL using latest tag redirect
  echo "${YELLOW}⚠️ GitHub API rate limit reached, resolving via tag redirect...${RESET}"
  LATEST_TAG=$(curl -s -L -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest" | rev | cut -d'/' -f1 | rev)
  TAG_CLEAN="${LATEST_TAG#v}"
  DMG_NAME="${APP_NAME}_${TAG_CLEAN}_${ARCH_KEY}.dmg"
  DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/${DMG_NAME}"
else
  # Extract DMG download url for matching arch
  DOWNLOAD_URL=$(echo "$RELEASE_JSON" | grep "browser_download_url" | grep -i "\.dmg\"" | grep -i "${ARCH_KEY}" | head -n 1 | cut -d '"' -f 4 || echo "")

  # Fallback if specific arch tag wasn't in filename or universal dmg
  if [ -z "$DOWNLOAD_URL" ]; then
    DOWNLOAD_URL=$(echo "$RELEASE_JSON" | grep "browser_download_url" | grep -i "\.dmg\"" | head -n 1 | cut -d '"' -f 4 || echo "")
  fi
fi

if [ -z "$DOWNLOAD_URL" ]; then
  echo "${RED}❌ Could not find a suitable .dmg release asset for ${ARCH_KEY}.${RESET}"
  echo "Please check https://github.com/${REPO}/releases manually."
  exit 1
fi

# 3. Download the DMG
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

DMG_PATH="${TMP_DIR}/Twominal.dmg"
echo "⬇️  Downloading ${APP_NAME} (${DOWNLOAD_URL})..."
curl -fSL --progress-bar "$DOWNLOAD_URL" -o "$DMG_PATH"

# 4. Mount DMG and Copy App
MOUNT_DIR="${TMP_DIR}/mount"
mkdir -p "$MOUNT_DIR"
echo "📦 Mounting installer..."
hdiutil attach "$DMG_PATH" -nobrowse -quiet -mountpoint "$MOUNT_DIR"

INSTALL_DIR="/Applications"
if [ ! -w "$INSTALL_DIR" ]; then
  INSTALL_DIR="${HOME}/Applications"
  mkdir -p "$INSTALL_DIR"
fi

echo "🚀 Installing to ${INSTALL_DIR}/${APP_NAME}.app..."
rm -rf "${INSTALL_DIR}/${APP_NAME}.app"
cp -R "${MOUNT_DIR}/${APP_NAME}.app" "${INSTALL_DIR}/"

# Unmount DMG
hdiutil detach "$MOUNT_DIR" -quiet || true

# 5. Remove Quarantine & Ad-Hoc Code Sign to bypass Gatekeeper
echo "🔓 Removing macOS quarantine attribute & applying local signature..."
xattr -cr "${INSTALL_DIR}/${APP_NAME}.app" 2>/dev/null || true
codesign --force --deep --sign - "${INSTALL_DIR}/${APP_NAME}.app" 2>/dev/null || true

echo "\n${GREEN}${BOLD}🎉 ${APP_NAME} installed successfully!${RESET}"
echo "You can open it from Spotlight, Finder, or by running:"
echo "\n    ${CYAN}open \"${INSTALL_DIR}/${APP_NAME}.app\"${RESET}\n"
