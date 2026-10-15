#!/bin/zsh
set -euo pipefail

PROJECT_ROOT="${0:A:h:h}"
BUILD_ROOT="$PROJECT_ROOT/.build"
RUST_TARGET="aarch64-apple-darwin"

# The DMG packager builds into its own temporary directory. Never allow an
# output override to reach another application or escape this project's build
# directory, including through a symlinked parent.
mkdir -p "$BUILD_ROOT"
OUTPUT_DIR="${CASKRUN_BUILD_OUTPUT_DIR:-$BUILD_ROOT}"
OUTPUT_DIR="$(cd -- "$OUTPUT_DIR" && pwd -P)"
if [[ "$OUTPUT_DIR" != "$BUILD_ROOT" && "$OUTPUT_DIR" != "$BUILD_ROOT/"* ]]; then
  print -u2 "Caskrun app output must stay inside $BUILD_ROOT"
  exit 64
fi
APP="$OUTPUT_DIR/Caskrun.app"
WORK="$(mktemp -d "$BUILD_ROOT/.caskrun-build.XXXXXXXX")"
STAGED_APP="$WORK/Caskrun.app"
PREVIOUS_APP="$WORK/previous.app"
SWAPPED=0

is_generated_app() {
  local candidate="$1" identifier
  [[ -d "$candidate" && ! -L "$candidate" &&
     -f "$candidate/Contents/Info.plist" &&
     -x "$candidate/Contents/MacOS/Caskrun" &&
     -x "$candidate/Contents/MacOS/caskrun-core" ]] || return 1
  identifier=$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$candidate/Contents/Info.plist" 2>/dev/null) || return 1
  [[ "$identifier" == dev.caskrun.app ]]
}

cleanup() {
  if [[ -e "$PREVIOUS_APP" || -L "$PREVIOUS_APP" ]]; then
    if (( ! SWAPPED )); then
      if [[ ! -e "$APP" && ! -L "$APP" ]]; then
        if ! mv "$PREVIOUS_APP" "$APP"; then
          print -u2 "Could not restore the previous app; it remains at $PREVIOUS_APP"
          return
        fi
      else
        print -u2 "Previous app remains at $PREVIOUS_APP because $APP already exists"
        return
      fi
    fi
  fi
  [[ ! -d "$WORK" ]] || rm -r "$WORK"
}
trap cleanup EXIT

# An older packaged app may contain its own engine. Check that it is one of
# our generated packages before replacing the app; never remove an unknown
# directory or an arbitrary Wine installation.
if [[ -e "$APP" || -L "$APP" ]]; then
  if ! is_generated_app "$APP"; then
    print -u2 "Refusing to replace an unrecognized app: $APP"
    exit 1
  fi
  BUNDLED_RUNTIME="$APP/Contents/Resources/wine-runtime"
  if [[ -e "$BUNDLED_RUNTIME" || -L "$BUNDLED_RUNTIME" ]]; then
    if [[ -L "$BUNDLED_RUNTIME" || ! -d "$BUNDLED_RUNTIME" ||
          ! -f "$BUNDLED_RUNTIME/SOURCES.txt" || ! -f "$BUNDLED_RUNTIME/bin/_dispatch" ]]; then
      print -u2 "Refusing to replace an unrecognized bundled runtime: $BUNDLED_RUNTIME"
      exit 1
    fi
  fi
  "$PROJECT_ROOT/scripts/assert-app-idle.sh" "$APP"
fi

cargo build --locked --release --target "$RUST_TARGET" --manifest-path "$PROJECT_ROOT/core/Cargo.toml"
SPARKLE_FRAMEWORK="$(python3 -B "$PROJECT_ROOT/scripts/prepare-sparkle.py" --print-framework)"
SPARKLE_PARENT="${SPARKLE_FRAMEWORK:h}"
mkdir -p "$STAGED_APP/Contents/MacOS" "$STAGED_APP/Contents/Resources"
cp "$PROJECT_ROOT/core/target/$RUST_TARGET/release/caskrun-core" "$STAGED_APP/Contents/MacOS/caskrun-core"
cp "$PROJECT_ROOT/assets/caskrun-icon.png" "$STAGED_APP/Contents/Resources/caskrun-icon.png"
cp "$PROJECT_ROOT/assets/caskrun-menubar-template.png" "$STAGED_APP/Contents/Resources/caskrun-menubar-template.png"
cp "$PROJECT_ROOT/LICENSE" "$STAGED_APP/Contents/Resources/LICENSE.txt"
python3 -B "$PROJECT_ROOT/scripts/package-rust-notices.py" "$STAGED_APP/Contents/Resources/RUST_DEPENDENCIES.txt"
python3 "$PROJECT_ROOT/catalog/validate.py"
mkdir -p "$STAGED_APP/Contents/Resources/catalog"
cp "$PROJECT_ROOT/catalog/apps.json" "$STAGED_APP/Contents/Resources/catalog/apps.json"
python3 -B "$PROJECT_ROOT/scripts/generate-game-compatibility.py" --check
python3 -B "$PROJECT_ROOT/catalog/validate_browser_games.py"
cp "$PROJECT_ROOT/catalog/game-compatibility.json" "$PROJECT_ROOT/catalog/browser-games.json" "$STAGED_APP/Contents/Resources/catalog/"
mkdir -p "$STAGED_APP/Contents/Resources/catalog/licenses"
cp "$PROJECT_ROOT/catalog/licenses/highball-db-CC0.txt" "$STAGED_APP/Contents/Resources/catalog/licenses/"
ICONSET="$WORK/CaskrunIcon.iconset"
mkdir -p "$ICONSET"
for SIZE in 16 32 128 256 512; do
  sips -s format png -z "$SIZE" "$SIZE" "$PROJECT_ROOT/assets/caskrun-icon.png" --out "$ICONSET/icon_${SIZE}x${SIZE}.png" >/dev/null
  DOUBLE=$((SIZE * 2))
  sips -s format png -z "$DOUBLE" "$DOUBLE" "$PROJECT_ROOT/assets/caskrun-icon.png" --out "$ICONSET/icon_${SIZE}x${SIZE}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$STAGED_APP/Contents/Resources/CaskrunIcon.icns"
swiftc -O -target arm64-apple-macos13.0 -parse-as-library "$PROJECT_ROOT/runtime-patches/native_icon.swift" -o "$STAGED_APP/Contents/MacOS/caskrun-native-icon"
swiftc -O -target arm64-apple-macos14.0 -parse-as-library -D CASKRUN_ICON_LIBRARY -F "$SPARKLE_PARENT" -framework Sparkle -Xlinker -rpath -Xlinker '@executable_path/../Frameworks' "$PROJECT_ROOT/app/CaskrunApp.swift" "$PROJECT_ROOT/app/WindowLifecycle.swift" "$PROJECT_ROOT/app/BottleActivity.swift" "$PROJECT_ROOT/app/AppUpdates.swift" "$PROJECT_ROOT/app/SteamGameArtwork.swift" "$PROJECT_ROOT/app/LaunchArguments.swift" "$PROJECT_ROOT/app/BottleMaintenanceViews.swift" "$PROJECT_ROOT/app/GameLibraryViews.swift" "$PROJECT_ROOT/app/BrowserLaunch.swift" "$PROJECT_ROOT/app/BrowserDiscoveryViews.swift" "$PROJECT_ROOT/app/CollectionViews.swift" "$PROJECT_ROOT/app/LocalArtwork.swift" "$PROJECT_ROOT/app/PS5Collection.swift" "$PROJECT_ROOT/app/PS5CollectionViews.swift" "$PROJECT_ROOT/app/GameCompatibility.swift" "$PROJECT_ROOT/app/CompatibilityBrowseViews.swift" "$PROJECT_ROOT/app/DXMTOptionsView.swift" "$PROJECT_ROOT/app/SteamGameLink.swift" "$PROJECT_ROOT/runtime-patches/native_icon.swift" -o "$STAGED_APP/Contents/MacOS/Caskrun"
mkdir -p "$STAGED_APP/Contents/Frameworks"
ditto "$SPARKLE_FRAMEWORK" "$STAGED_APP/Contents/Frameworks/Sparkle.framework"
cp "$SPARKLE_PARENT/LICENSE" "$STAGED_APP/Contents/Resources/SPARKLE_LICENSE.txt"
for BINARY in "$STAGED_APP/Contents/MacOS/Caskrun" "$STAGED_APP/Contents/MacOS/caskrun-core" "$STAGED_APP/Contents/MacOS/caskrun-native-icon"; do
  if [[ "$(lipo -archs "$BINARY")" != arm64 ]]; then
    print -u2 "Expected an ARM64-only binary: $BINARY"
    exit 1
  fi
done
cat > "$STAGED_APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Caskrun</string>
  <key>CFBundleDisplayName</key><string>Caskrun</string>
  <key>CFBundleIdentifier</key><string>dev.caskrun.app</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleExecutable</key><string>Caskrun</string>
  <key>CFBundleIconFile</key><string>CaskrunIcon</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Windows apps you open with Caskrun can use your microphone for voice chat or audio recording when you choose those features.</string>
</dict></plist>
PLIST
python3 -B "$PROJECT_ROOT/scripts/configure-app-updates.py" "$STAGED_APP/Contents/Info.plist"
touch "$STAGED_APP/Contents/Resources/.caskrun-generated-app"
codesign --force --deep --sign - "$STAGED_APP"
codesign --verify --deep --strict "$STAGED_APP"

if [[ -e "$APP" || -L "$APP" ]]; then
  "$PROJECT_ROOT/scripts/assert-app-idle.sh" "$APP"
  mv "$APP" "$PREVIOUS_APP"
fi
mv "$STAGED_APP" "$APP"
SWAPPED=1
echo "Built $APP"
