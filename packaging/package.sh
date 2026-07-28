#!/usr/bin/env bash
set -euo pipefail

LABEL="${1:?usage: package.sh <label> <target>}"
TARGET="${2:?usage: package.sh <label> <target>}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
NAME="game-studio-crew-$VERSION-$LABEL"

WORKSPACE_BIN="$ROOT/target/$TARGET/release"
SHELL_BIN="$ROOT/desktop/target/$TARGET/release"

DIST="$ROOT/dist"
STAGE="$ROOT/.package/$NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE" "$DIST"

copy_if_present() {
  if [ -f "$1" ]; then
    cp "$1" "$2"
    return 0
  fi
  return 1
}

case "$LABEL" in
  windows-*)
    cp "$WORKSPACE_BIN/studiod.exe" "$STAGE/"
    copy_if_present "$SHELL_BIN/game-studio.exe" "$STAGE/" || true
    cp "$ROOT/README.md" "$ROOT/LICENSE" "$STAGE/" 2>/dev/null || cp "$ROOT/README.md" "$STAGE/"
    (cd "$ROOT/.package" && 7z a -tzip "$DIST/$NAME.zip" "$NAME" >/dev/null)
    ;;

  macos-*)
    APP="$STAGE/Game Studio Crew.app"
    mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

    cp "$WORKSPACE_BIN/studiod" "$APP/Contents/MacOS/"
    copy_if_present "$SHELL_BIN/game-studio" "$APP/Contents/MacOS/" || \
      cp "$WORKSPACE_BIN/studiod" "$APP/Contents/MacOS/game-studio"
    chmod +x "$APP/Contents/MacOS/"*

    cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Game Studio Crew</string>
  <key>CFBundleDisplayName</key><string>Game Studio Crew</string>
  <key>CFBundleIdentifier</key><string>dev.gamestudiocrew.shell</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>game-studio</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

    cp "$ROOT/README.md" "$STAGE/"
    cp "$ROOT/LICENSE" "$STAGE/" 2>/dev/null || true
    (cd "$ROOT/.package" && tar czf "$DIST/$NAME.tar.gz" "$NAME")

    if command -v hdiutil >/dev/null 2>&1; then
      DMG_ROOT="$ROOT/.package/dmg-$LABEL"
      rm -rf "$DMG_ROOT"
      mkdir -p "$DMG_ROOT"
      cp -R "$APP" "$DMG_ROOT/"
      ln -s /Applications "$DMG_ROOT/Applications"
      cp "$ROOT/README.md" "$DMG_ROOT/"
      hdiutil create \
        -volname "Game Studio Crew" \
        -srcfolder "$DMG_ROOT" \
        -ov -format UDZO \
        "$DIST/$NAME.dmg" >/dev/null
    fi
    ;;

  linux-*)
    mkdir -p "$STAGE/bin" "$STAGE/share/applications" "$STAGE/share/icons"

    cp "$WORKSPACE_BIN/studiod" "$STAGE/bin/"
    copy_if_present "$SHELL_BIN/game-studio" "$STAGE/bin/" || true
    chmod +x "$STAGE/bin/"*

    cp "$ROOT/images/logo.png" "$STAGE/share/icons/game-studio-crew.png"

    cat > "$STAGE/share/applications/game-studio-crew.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Game Studio Crew
Comment=A game studio that runs itself
Exec=game-studio
Icon=game-studio-crew
Terminal=false
Categories=Development;IDE;
DESKTOP

    cat > "$STAGE/install.sh" <<'INSTALL'
#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"

mkdir -p "$PREFIX/bin" "$PREFIX/share/applications" "$PREFIX/share/icons/hicolor/512x512/apps"
install -m755 "$HERE/bin/"* "$PREFIX/bin/"
install -m644 "$HERE/share/applications/game-studio-crew.desktop" "$PREFIX/share/applications/"
install -m644 "$HERE/share/icons/game-studio-crew.png" "$PREFIX/share/icons/hicolor/512x512/apps/"

echo "Installed to $PREFIX. Make sure $PREFIX/bin is on your PATH."
echo "Run 'studiod doctor' to see what the studio still needs."
INSTALL
    chmod +x "$STAGE/install.sh"

    cp "$ROOT/README.md" "$STAGE/"
    cp "$ROOT/LICENSE" "$STAGE/" 2>/dev/null || true
    (cd "$ROOT/.package" && tar czf "$DIST/$NAME.tar.gz" "$NAME")
    ;;

  *)
    echo "unknown package label: $LABEL" >&2
    exit 1
    ;;
esac

echo "packaged $NAME"
ls -la "$DIST"
