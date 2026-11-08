#!/usr/bin/env bash
set -e

# Rnote macOS DMG Packaging & Notarization Script
# Usage:
#   ./build-aux/macos/package_dmg.sh [OPTIONS]
# Options:
#   --sign "<IDENTITY>"        Sign DMG with codesign identity
#   --notarize "<KEYCHAIN_PROFILE>" Submit to Apple notarytool and staple

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$ROOT_DIR"

APP_PATH="${APP_PATH:-_build/Rnote.app}"
if [ ! -d "$APP_PATH" ] && [ -d "/Applications/Rnote.app" ]; then
    APP_PATH="/Applications/Rnote.app"
fi

if [ ! -d "$APP_PATH" ]; then
    echo "Error: Rnote.app not found at _build/Rnote.app or /Applications/Rnote.app."
    echo "Please run ./build-aux/macos/build_local.sh first."
    exit 1
fi

VERSION="0.15.0"
DMG_NAME="Rnote-${VERSION}-macOS-arm64.dmg"
DMG_OUTPUT="${ROOT_DIR}/${DMG_NAME}"
STAGING_DIR="/tmp/rnote_dmg_staging"

SIGN_IDENTITY=""
KEYCHAIN_PROFILE=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --sign)
            SIGN_IDENTITY="$2"
            shift 2
            ;;
        --notarize)
            KEYCHAIN_PROFILE="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1"
            exit 1
            ;;
    esac
done

echo "==> Preparing DMG staging directory..."
rm -rf "$STAGING_DIR"
mkdir -p "$STAGING_DIR"

echo "==> Copying $APP_PATH..."
cp -R "$APP_PATH" "$STAGING_DIR/Rnote.app"

echo "==> Creating /Applications symlink..."
ln -s /Applications "$STAGING_DIR/Applications"

echo "==> Creating compressed DMG with hdiutil..."
rm -f "$DMG_OUTPUT"
hdiutil create \
    -volname "Rnote ${VERSION}" \
    -srcfolder "$STAGING_DIR" \
    -ov \
    -format UDZO \
    "$DMG_OUTPUT"

rm -rf "$STAGING_DIR"

echo "==> Successfully created DMG: $DMG_OUTPUT"

if [ -n "$SIGN_IDENTITY" ]; then
    echo "==> Signing DMG with identity: $SIGN_IDENTITY..."
    codesign --timestamp --sign "$SIGN_IDENTITY" "$DMG_OUTPUT"
    echo "==> Verifying DMG signature..."
    codesign --verify --verbose=2 "$DMG_OUTPUT"
fi

if [ -n "$KEYCHAIN_PROFILE" ]; then
    echo "==> Submitting to Apple Notary Service via notarytool..."
    xcrun notarytool submit "$DMG_OUTPUT" --keychain-profile "$KEYCHAIN_PROFILE" --wait
    echo "==> Stapling notarization ticket to DMG..."
    xcrun stapler staple "$DMG_OUTPUT"
    echo "==> DMG successfully notarized and stapled!"
fi

echo "==> All packaging steps completed."
