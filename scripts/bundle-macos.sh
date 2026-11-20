#!/bin/sh
# Build Oxide.app: a real macOS app, so the Dock, menu bar, Finder and
# Spotlight show "Oxide" with its icon (a bare binary shows "oxide-app").
#
#   scripts/bundle-macos.sh            build → target/release/Oxide.app
#   scripts/bundle-macos.sh --install  also copy it to ~/Applications
#
# The icon comes from assets/Oxide.png (1024×1024); replace it to change the icon.
set -eu
cd "$(dirname "$0")/.."
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"

"$CARGO" build --release -p oxide-app

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/oxide-app/Cargo.toml | head -1)
APP=target/release/Oxide.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/oxide-app "$APP/Contents/MacOS/oxide-app"

# Icon: every size macOS wants, packed into an .icns.
ICONSET=target/release/Oxide.iconset
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z $size $size assets/Oxide.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double assets/Oxide.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Oxide.icns"
rm -rf "$ICONSET"

# Help → Oxide Help: copy the wiki's pages (read only; the wiki isn't
# touched) and the pictures the Using Oxide pages show. Set OXIDE_WIKI to use
# a wiki clone somewhere else. Without one, Help offers the online wiki.
WIKI="${OXIDE_WIKI:-../Oxide.wiki/Oxide.wiki}"
if [ -f "$WIKI/Home.md" ]; then
    HELP="$APP/Contents/Resources/help"
    mkdir -p "$HELP/Attachments"
    cp "$WIKI"/*.md "$HELP/"
    # The pages linked from Home's "Using Oxide" section, and their images.
    USING=$(sed -n '/^## Using Oxide/,/^## /p' "$WIKI/Home.md" \
        | grep -o '\[\[[^]]*\]\]' | sed 's/^\[\[//; s/\]\]$//; s/.*|//; s/ /-/g' | sort -u)
    for page in $USING; do
        [ -f "$WIKI/$page.md" ] || continue
        grep -o 'Attachments/[^)" ]*' "$WIKI/$page.md" | sort -u | while read -r pic; do
            if [ -f "$WIKI/$pic" ]; then cp "$WIKI/$pic" "$HELP/Attachments/"; fi
        done || true
    done
    echo "Bundled help from $WIKI"
else
    echo "No wiki clone at $WIKI: Help will offer the online wiki"
fi

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>             <string>Oxide</string>
    <key>CFBundleDisplayName</key>      <string>Oxide</string>
    <key>CFBundleIdentifier</key>       <string>com.kevingaddy.oxide</string>
    <key>CFBundleExecutable</key>       <string>oxide-app</string>
    <key>CFBundleIconFile</key>         <string>Oxide</string>
    <key>CFBundlePackageType</key>      <string>APPL</string>
    <key>CFBundleVersion</key>          <string>$VERSION</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
    <key>LSMinimumSystemVersion</key>   <string>11.0</string>
    <key>NSHighResolutionCapable</key>  <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
    <!-- .oxide paintings open in Oxide (double-click, Open With); brush files and images can be opened with it too. -->
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>        <string>Oxide Painting</string>
            <key>CFBundleTypeRole</key>        <string>Editor</string>
            <key>LSHandlerRank</key>           <string>Owner</string>
            <key>CFBundleTypeIconFile</key>    <string>Oxide</string>
            <key>LSItemContentTypes</key>      <array><string>com.kevingaddy.oxide.painting</string></array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key>        <string>Brush Tips</string>
            <key>CFBundleTypeRole</key>        <string>Viewer</string>
            <key>LSHandlerRank</key>           <string>Alternate</string>
            <key>CFBundleTypeExtensions</key>  <array><string>abr</string><string>brush</string><string>brushset</string><string>gbr</string><string>gih</string></array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key>        <string>Image</string>
            <key>CFBundleTypeRole</key>        <string>Viewer</string>
            <key>LSHandlerRank</key>           <string>Alternate</string>
            <key>LSItemContentTypes</key>      <array><string>public.image</string></array>
        </dict>
    </array>
    <key>UTExportedTypeDeclarations</key>
    <array>
        <dict>
            <key>UTTypeIdentifier</key>        <string>com.kevingaddy.oxide.painting</string>
            <key>UTTypeDescription</key>       <string>Oxide Painting</string>
            <key>UTTypeIconFile</key>          <string>Oxide</string>
            <key>UTTypeConformsTo</key>        <array><string>public.data</string><string>public.content</string></array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key> <array><string>oxide</string></array>
                <key>public.mime-type</key>          <array><string>application/vnd.oxide+zip</string></array>
            </dict>
        </dict>
    </array>
</dict>
</plist>
PLIST

# Ad-hoc signature, so macOS treats it as one consistent app.
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true
echo "Built $APP"

if [ "${1:-}" = "--install" ]; then
    mkdir -p "$HOME/Applications"
    rm -rf "$HOME/Applications/Oxide.app"
    cp -R "$APP" "$HOME/Applications/Oxide.app"
    # Tell Finder about the app (and the .oxide file type) right away.
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$HOME/Applications/Oxide.app" >/dev/null 2>&1 || true
    echo "Installed ~/Applications/Oxide.app"
fi
