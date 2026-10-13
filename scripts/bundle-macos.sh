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
    echo "Installed ~/Applications/Oxide.app"
fi
