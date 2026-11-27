#!/bin/bash
set -e

echo "=== Building XDM-Rust AppImage for Linux (x86_64) ==="

# 1. Compile release binary
cargo build --release

# 2. Setup AppDir directory layout
APPDIR="XDM-Rust.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"

# 3. Copy payload
cp target/release/xdm-rust "$APPDIR/usr/bin/xdm-rust"
chmod +x "$APPDIR/usr/bin/xdm-rust"

cp xdm-rust.desktop "$APPDIR/xdm-rust.desktop"
cp xdm-rust.desktop "$APPDIR/usr/share/applications/xdm-rust.desktop"

cp app_icon.png "$APPDIR/xdm-rust.png"
cp app_icon.png "$APPDIR/.DirIcon"
cp app_icon.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/xdm-rust.png"

cp AppRun "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"

# 4. Check for appimagetool or download it
if ! command -v appimagetool &> /dev/null; then
    if [ ! -f "appimagetool-x86_64.AppImage" ]; then
        echo "Downloading appimagetool..."
        curl -fsSL https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage -o appimagetool-x86_64.AppImage
        chmod +x appimagetool-x86_64.AppImage
    fi
    ARCH=x86_64 ./appimagetool-x86_64.AppImage "$APPDIR" XDM-Rust-1.0.0-x86_64.AppImage
else
    ARCH=x86_64 appimagetool "$APPDIR" XDM-Rust-1.0.0-x86_64.AppImage
fi

echo "=== AppImage created successfully: XDM-Rust-1.0.0-x86_64.AppImage ==="
