#!/usr/bin/env bash
set -e

# Setup environment
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/opt/homebrew/opt/gettext/bin:$PATH"
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:/opt/homebrew/opt/libadwaita/lib/pkgconfig:/opt/homebrew/opt/libadwaita-1/lib/pkgconfig:$PKG_CONFIG_PATH"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

echo "==> Configuring Meson build..."
if [ ! -f "_build/build.ninja" ]; then
    meson setup _build --prefix=/tmp/rnote_install -Dprofile=default
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

echo "==> Building Quick Look & Thumbnail Extensions..."
PLUGINS_DIR="$APP_DIR/Contents/PlugIns"
mkdir -p "$PLUGINS_DIR/RnoteQuickLookPreview.appex/Contents/MacOS"
mkdir -p "$PLUGINS_DIR/RnoteQuickLookThumbnail.appex/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Library/QuickLook/RnoteQuickLook.qlgenerator/Contents/MacOS"

# 1. Preview App Extension
clang -fmodules -framework AppKit -framework QuickLookUI -framework Foundation -framework CoreGraphics -lz \
    -e _NSExtensionMain \
    build-aux/macos/quicklook/RnoteQuickLookPreview.m \
    -o "$PLUGINS_DIR/RnoteQuickLookPreview.appex/Contents/MacOS/RnoteQuickLookPreview"

cat << 'EOF' > "$PLUGINS_DIR/RnoteQuickLookPreview.appex/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>Rnote Quick Look Preview</string>
    <key>CFBundleExecutable</key>
    <string>RnoteQuickLookPreview</string>
    <key>CFBundleIdentifier</key>
    <string>com.github.flxzt.rnote.quicklookpreview</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>RnoteQuickLookPreview</string>
    <key>CFBundlePackageType</key>
    <string>XPC!</string>
    <key>CFBundleShortVersionString</key>
    <string>0.15.0</string>
    <key>CFBundleVersion</key>
    <string>0.15.0</string>
    <key>NSExtension</key>
    <dict>
        <key>NSExtensionAttributes</key>
        <dict>
            <key>QLIsDataBasedPreview</key>
            <false/>
            <key>QLSupportedContentTypes</key>
            <array>
                <string>com.github.flxzt.rnote.document</string>
            </array>
            <key>QLSupportsSearchableItems</key>
            <false/>
        </dict>
        <key>NSExtensionPointIdentifier</key>
        <string>com.apple.quicklook.preview</string>
        <key>NSExtensionPrincipalClass</key>
        <string>RnotePreviewViewController</string>
    </dict>
</dict>
</plist>
EOF

# 2. Thumbnail App Extension
clang -fmodules -framework AppKit -framework QuickLookThumbnailing -framework Foundation -framework CoreGraphics -lz \
    -e _NSExtensionMain \
    build-aux/macos/quicklook/RnoteQuickLookThumbnail.m \
    -o "$PLUGINS_DIR/RnoteQuickLookThumbnail.appex/Contents/MacOS/RnoteQuickLookThumbnail"

cat << 'EOF' > "$PLUGINS_DIR/RnoteQuickLookThumbnail.appex/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>Rnote Quick Look Thumbnail</string>
    <key>CFBundleExecutable</key>
    <string>RnoteQuickLookThumbnail</string>
    <key>CFBundleIdentifier</key>
    <string>com.github.flxzt.rnote.quicklookthumbnail</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>RnoteQuickLookThumbnail</string>
    <key>CFBundlePackageType</key>
    <string>XPC!</string>
    <key>CFBundleShortVersionString</key>
    <string>0.15.0</string>
    <key>CFBundleVersion</key>
    <string>0.15.0</string>
    <key>NSExtension</key>
    <dict>
        <key>NSExtensionAttributes</key>
        <dict>
            <key>QLSupportedContentTypes</key>
            <array>
                <string>com.github.flxzt.rnote.document</string>
            </array>
            <key>QLThumbnailMinimumDimension</key>
            <integer>0</integer>
        </dict>
        <key>NSExtensionPointIdentifier</key>
        <string>com.apple.quicklook.thumbnail</string>
        <key>NSExtensionPrincipalClass</key>
        <string>RnoteThumbnailProvider</string>
    </dict>
</dict>
</plist>
EOF

# 3. Fallback QuickLook Generator (CFPlugin)
clang -bundle -framework Foundation -framework QuickLook -framework CoreFoundation -framework ApplicationServices \
    build-aux/macos/quicklook/RnoteQuickLookGenerator.m \
    -o "$APP_DIR/Contents/Library/QuickLook/RnoteQuickLook.qlgenerator/Contents/MacOS/RnoteQuickLook"

cat << 'EOF' > "$APP_DIR/Contents/Library/QuickLook/RnoteQuickLook.qlgenerator/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeRole</key>
            <string>QLGenerator</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.github.flxzt.rnote.document</string>
            </array>
        </dict>
    </array>
    <key>CFBundleExecutable</key>
    <string>RnoteQuickLook</string>
    <key>CFBundleIdentifier</key>
    <string>com.github.flxzt.rnote.quicklook</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>RnoteQuickLook</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.15.0</string>
    <key>CFBundleVersion</key>
    <string>0.15.0</string>
    <key>CFPlugInDynamicRegisterFunction</key>
    <string></string>
    <key>CFPlugInDynamicRegistration</key>
    <string>NO</string>
    <key>CFPlugInFactories</key>
    <dict>
        <key>B69A5EB5-A3E9-4670-87C2-7BDDFB1A1234</key>
        <string>QuickLookGeneratorPluginFactory</string>
    </dict>
    <key>CFPlugInTypes</key>
    <dict>
        <key>5E2D9680-5022-40FA-B806-43349622E5B9</key>
        <array>
            <string>B69A5EB5-A3E9-4670-87C2-7BDDFB1A1234</string>
        </array>
    </dict>
    <key>CFPlugInUnloadFunction</key>
    <string></string>
    <key>QLNeedsToBeRunInMainThread</key>
    <false/>
    <key>QLPreviewHeight</key>
    <real>900</real>
    <key>QLPreviewWidth</key>
    <real>700</real>
    <key>QLSupportsConcurrentRequests</key>
    <true/>
    <key>QLThumbnailMinimumSize</key>
    <real>17</real>
</dict>
</plist>
EOF

# Codesign plugins with sandbox entitlements required by macOS pluginkit
codesign -s - --force --entitlements build-aux/macos/appex.entitlements "$PLUGINS_DIR/RnoteQuickLookPreview.appex" 2>/dev/null || true
codesign -s - --force --entitlements build-aux/macos/appex.entitlements "$PLUGINS_DIR/RnoteQuickLookThumbnail.appex" 2>/dev/null || true
codesign -s - --force "$APP_DIR/Contents/Library/QuickLook/RnoteQuickLook.qlgenerator" 2>/dev/null || true
codesign -s - --force "$APP_DIR" 2>/dev/null || true

if [ "$1" = "--install" ]; then
    echo "==> Installing to /Applications/Rnote.app..."
    pkill -9 Rnote || true
    rm -rf /Applications/Rnote.app
    cp -R "$APP_DIR" /Applications/
    xattr -dr com.apple.quarantine /Applications/Rnote.app 2>/dev/null || true

    mkdir -p "$HOME/Library/QuickLook"
    rm -rf "$HOME/Library/QuickLook/RnoteQuickLook.qlgenerator"
    cp -R "$APP_DIR/Contents/Library/QuickLook/RnoteQuickLook.qlgenerator" "$HOME/Library/QuickLook/"

    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /Applications/Rnote.app
    pluginkit -a /Applications/Rnote.app/Contents/PlugIns/RnoteQuickLookPreview.appex 2>/dev/null || true
    pluginkit -a /Applications/Rnote.app/Contents/PlugIns/RnoteQuickLookThumbnail.appex 2>/dev/null || true
    qlmanage -r
    qlmanage -r cache
    echo "==> Successfully installed to /Applications/Rnote.app!"
fi

echo "==> Done!"
