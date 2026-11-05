#!/usr/bin/env bash
set -e

# Setup environment
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/opt/homebrew/opt/gettext/bin:$PATH"
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:/opt/homebrew/opt/libadwaita/lib/pkgconfig:/opt/homebrew/opt/libadwaita-1/lib/pkgconfig:$PKG_CONFIG_PATH"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

echo "==> Configuring Meson build..."
if [ ! -d "_build" ]; then
    meson setup _build --prefix=/tmp/rnote_install -Dprofile=default
else
    meson setup _build --reconfigure --prefix=/tmp/rnote_install -Dprofile=default
fi

echo "==> Compiling Rnote..."
ninja -C _build

echo "==> Installing to staging..."
ninja -C _build install

echo "==> Packaging Rnote.app..."
APP_DIR="_build/Rnote.app"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources/share"

cp /tmp/rnote_install/bin/rnote "$APP_DIR/Contents/MacOS/Rnote"
if [ -f /tmp/rnote_install/bin/rnote-cli ]; then
    cp /tmp/rnote_install/bin/rnote-cli "$APP_DIR/Contents/MacOS/rnote-cli"
fi

cp -r /tmp/rnote_install/share/* "$APP_DIR/Contents/Resources/share/"
cp /opt/homebrew/share/glib-2.0/schemas/org.gtk* "$APP_DIR/Contents/Resources/share/glib-2.0/schemas/" 2>/dev/null || true
cp /opt/homebrew/share/glib-2.0/schemas/org.gnome* "$APP_DIR/Contents/Resources/share/glib-2.0/schemas/" 2>/dev/null || true
glib-compile-schemas "$APP_DIR/Contents/Resources/share/glib-2.0/schemas" 2>/dev/null || true

mkdir -p "$APP_DIR/Contents/Resources/etc"
if [ -f /opt/homebrew/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache ]; then
    cp /opt/homebrew/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache "$APP_DIR/Contents/Resources/etc/loaders.cache"
fi

cp build-aux/macos/Rnote.icns "$APP_DIR/Contents/Resources/Rnote.icns"
cp build-aux/macos/RnoteDoc.icns "$APP_DIR/Contents/Resources/RnoteDoc.icns"

cat << 'EOF' > "$APP_DIR/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleIdentifier</key>
    <string>com.github.flxzt.rnote</string>
    <key>CFBundleName</key>
    <string>Rnote</string>
    <key>CFBundleDisplayName</key>
    <string>Rnote</string>
    <key>CFBundleExecutable</key>
    <string>Rnote</string>
    <key>CFBundleIconFile</key>
    <string>Rnote</string>
    <key>CFBundleVersion</key>
    <string>0.15.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.15.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSRequiresAquaSystemAppearance</key>
    <false/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Rnote Document</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
            <key>LSHandlerRank</key>
            <string>Owner</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.github.flxzt.rnote.document</string>
            </array>
            <key>CFBundleTypeIconFile</key>
            <string>RnoteDoc</string>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>rnote</string>
            </array>
        </dict>
    </array>
    <key>UTExportedTypeDeclarations</key>
    <array>
        <dict>
            <key>UTTypeIdentifier</key>
            <string>com.github.flxzt.rnote.document</string>
            <key>UTTypeDescription</key>
            <string>Rnote Document</string>
            <key>UTTypeConformsTo</key>
            <array>
                <string>public.data</string>
                <string>public.content</string>
            </array>
            <key>UTTypeIconFile</key>
            <string>RnoteDoc</string>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key>
                <array>
                    <string>rnote</string>
                </array>
                <key>public.mime-type</key>
                <array>
                    <string>application/rnote</string>
                </array>
            </dict>
        </dict>
    </array>
</dict>
</plist>
EOF

if [ "$1" = "--install" ]; then
    echo "==> Installing to /Applications/Rnote.app..."
    pkill -9 Rnote || true
    rm -rf /Applications/Rnote.app
    cp -R "$APP_DIR" /Applications/
    xattr -dr com.apple.quarantine /Applications/Rnote.app 2>/dev/null || true
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /Applications/Rnote.app
    echo "==> Successfully installed to /Applications/Rnote.app!"
fi

echo "==> Done!"
