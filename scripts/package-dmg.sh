#!/bin/zsh
set -euo pipefail

PROJECT_ROOT="${0:A:h:h}"
BUILD_ROOT="${CASKRUN_PACKAGE_OUTPUT_DIR:-$PROJECT_ROOT/.build}"
APP="$BUILD_ROOT/Caskrun.app"
# A separate output directory lets us prepare a candidate while an existing
# Caskrun app is still serving a running Windows process.
case "${CASKRUN_PACKAGE_RUNTIME_VARIANT:-native-apps}" in
  steam) RUNTIME="$PROJECT_ROOT/.build/steam-wine-runtime-dev" ;;
  native-apps)
    NATIVE_RUNTIME_NAME="${CASKRUN_PACKAGE_NATIVE_RUNTIME_NAME:-dxmt-wine-runtime-dev-v4}"
    case "$NATIVE_RUNTIME_NAME" in
      ''|.*|*/*) print -u2 "Invalid native runtime package name."; exit 64 ;;
    esac
    RUNTIME="$PROJECT_ROOT/.build/$NATIVE_RUNTIME_NAME"
    ;;
  *) print -u2 "Unknown independent runtime package variant."; exit 64 ;;
esac
LEGACY_STAGE="$BUILD_ROOT/dmg-stage"

is_generated_app() {
  local candidate="$1" identifier
  [[ -d "$candidate" && ! -L "$candidate" &&
     -f "$candidate/Contents/Info.plist" &&
     -x "$candidate/Contents/MacOS/Caskrun" &&
     -x "$candidate/Contents/MacOS/caskrun-core" ]] || return 1
  identifier=$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$candidate/Contents/Info.plist" 2>/dev/null) || return 1
  [[ "$identifier" == dev.caskrun.app ]]
}

if [[ -L "$RUNTIME" || ! -d "$RUNTIME" ||
      ! -x "$RUNTIME/bin/wine" || ! -x "$RUNTIME/bin/wineboot" ||
      ! -f "$RUNTIME/bin/_dispatch" || ! -f "$RUNTIME/SOURCES.txt" ||
      ! -x "$RUNTIME/runtime/lib/wine/x86_64-unix/Steam" ||
      ! -f "$RUNTIME/source/SOURCES.txt" ]]; then
  print -u2 "Missing complete independent development engine: $RUNTIME"
  print -u2 "Build the reviewed runtime described in docs/DXMT_APP_INTEGRATION.md before packaging Caskrun."
  exit 66
fi
if [[ "${CASKRUN_PACKAGE_RUNTIME_VARIANT:-native-apps}" == native-apps ]]; then
  if [[ ! -x "$RUNTIME/runtime/lib/wine/x86_64-unix/WindowsApp" ||
        ! -x "$RUNTIME/bin/native-icon" || ! -f "$RUNTIME/native-default-icon.png" ||
        ! -f "$RUNTIME/native-default-icon.icns" || ! -f "$RUNTIME/native-default-rendered.png" ||
        "$(cat "$RUNTIME/native-app-identity.txt")" != caskrun-native-app-identity-v1 ]]; then
    print -u2 "Missing complete per-EXE native app identity package."; exit 66
  fi
fi
if [[ -e "$APP" || -L "$APP" ]] && ! is_generated_app "$APP"; then
  print -u2 "Refusing to replace an unrecognized app: $APP"
  exit 1
fi
if [[ -e "$APP/Contents/Resources/wine-runtime" || -L "$APP/Contents/Resources/wine-runtime" ]]; then
  EXISTING_RUNTIME="$APP/Contents/Resources/wine-runtime"
  if [[ -L "$EXISTING_RUNTIME" || ! -d "$EXISTING_RUNTIME" ||
        ! -f "$EXISTING_RUNTIME/SOURCES.txt" || ! -f "$EXISTING_RUNTIME/bin/_dispatch" ]]; then
    print -u2 "Refusing to replace an unrecognized bundled runtime: $EXISTING_RUNTIME"
    exit 1
  fi
fi
"$PROJECT_ROOT/scripts/assert-app-idle.sh" "$APP"

# Older versions used a persistent staging directory. Only remove that known
# generated directory; the new package is staged privately and cleaned up.
if [[ -e "$LEGACY_STAGE" || -L "$LEGACY_STAGE" ]]; then
  if [[ -L "$LEGACY_STAGE" || ! -d "$LEGACY_STAGE" || ! -f "$LEGACY_STAGE/.caskrun-dmg-stage" ]]; then
    print -u2 "Refusing to replace an unrecognized DMG staging folder: $LEGACY_STAGE"
    exit 1
  fi
  rm -r "$LEGACY_STAGE"
fi

mkdir -p "$BUILD_ROOT"
WORK="$(mktemp -d "$BUILD_ROOT/.caskrun-package.XXXXXXXX")"
STAGED_APP="$WORK/Caskrun.app"
STAGE="$WORK/dmg-stage"
PREVIOUS_APP="$WORK/previous.app"
PREVIOUS_DMG="$WORK/previous.dmg"
PREVIOUS_SHA="$WORK/previous.dmg.sha256"
NEW_APP=0
NEW_DMG=0
NEW_SHA=0
COMMITTED=0

cleanup() {
  local rollback_ok=1
  if (( ! COMMITTED )); then
    if (( NEW_SHA )) && [[ -e "$SHA" || -L "$SHA" ]]; then
      rm -f "$SHA" || rollback_ok=0
    fi
    if (( NEW_DMG )) && [[ -e "$DMG" || -L "$DMG" ]]; then
      rm -f "$DMG" || rollback_ok=0
    fi
    if (( NEW_APP )) && [[ -e "$APP" || -L "$APP" ]]; then
      if is_generated_app "$APP"; then
        rm -r "$APP" || rollback_ok=0
      else
        rollback_ok=0
      fi
    fi
    if [[ -e "$PREVIOUS_SHA" || -L "$PREVIOUS_SHA" ]]; then
      if [[ ! -e "$SHA" && ! -L "$SHA" ]]; then
        mv "$PREVIOUS_SHA" "$SHA" || rollback_ok=0
      else
        rollback_ok=0
      fi
    fi
    if [[ -e "$PREVIOUS_DMG" || -L "$PREVIOUS_DMG" ]]; then
      if [[ ! -e "$DMG" && ! -L "$DMG" ]]; then
        mv "$PREVIOUS_DMG" "$DMG" || rollback_ok=0
      else
        rollback_ok=0
      fi
    fi
    if [[ -e "$PREVIOUS_APP" || -L "$PREVIOUS_APP" ]]; then
      if [[ ! -e "$APP" && ! -L "$APP" ]]; then
        mv "$PREVIOUS_APP" "$APP" || rollback_ok=0
      else
        rollback_ok=0
      fi
    fi
  fi
  if (( rollback_ok )) && [[ -d "$WORK" ]]; then
    rm -r "$WORK"
  elif (( ! rollback_ok )); then
    print -u2 "Could not fully restore the previous package; preserved artifacts at $WORK"
  fi
}
trap cleanup EXIT

# Build and sign the entire candidate without changing the visible app.
CASKRUN_BUILD_OUTPUT_DIR="$WORK" "$PROJECT_ROOT/scripts/build.sh"
ditto "$RUNTIME" "$STAGED_APP/Contents/Resources/wine-runtime"
codesign --force --deep --sign - "$STAGED_APP"
codesign --verify --deep --strict "$STAGED_APP"
if [[ "$(lipo -archs "$STAGED_APP/Contents/MacOS/Caskrun")" != arm64 ||
      "$(lipo -archs "$STAGED_APP/Contents/MacOS/caskrun-core")" != arm64 ||
      "$(lipo -archs "$STAGED_APP/Contents/Resources/wine-runtime/runtime/bin/wine")" != x86_64 ]]; then
  print -u2 "Unexpected app or Windows engine architecture."
  exit 65
fi
VERSION=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$STAGED_APP/Contents/Info.plist")
DMG="$BUILD_ROOT/Caskrun-$VERSION-arm64-dev.dmg"
SHA="$DMG.sha256"
TEMP_DMG="$WORK/Caskrun-$VERSION-arm64-dev.dmg"
TEMP_SHA="$TEMP_DMG.sha256"

if [[ -e "$DMG" || -L "$DMG" || -e "$SHA" || -L "$SHA" ]]; then
  if [[ -L "$DMG" || -L "$SHA" || ! -f "$DMG" || ! -f "$SHA" ]] ||
     ! shasum -a 256 -c "$SHA" >/dev/null; then
    print -u2 "Refusing to replace an unrecognized or unverified development DMG: $DMG"
    exit 1
  fi
fi

mkdir -p "$STAGE"
touch "$STAGE/.caskrun-dmg-stage"
ditto "$STAGED_APP" "$STAGE/Caskrun.app"
ln -s /Applications "$STAGE/Applications"
cp "$PROJECT_ROOT/LICENSE" "$STAGE/LICENSE.txt"
cat > "$STAGE/READ ME.txt" <<'NOTE'
Caskrun development build

This app includes an independent Wine development runtime. The Caskrun interface
and bottle manager are ARM64; the x86_64 Windows compatibility engine runs via
Apple Rosetta. No Wine selection is needed in the app's normal workflow.

Store installation and sign-in, graphics, media, controllers, and game
compatibility are still experimental or unverified. This build does
not establish that a particular game will run.

This local development build is ad-hoc signed and is not notarized for public
distribution. Engine source and license notices are inside Caskrun.app at
Contents/Resources/wine-runtime. Source and build instructions are in the
Caskrun project folder.
NOTE

hdiutil create -srcfolder "$STAGE" -volname Caskrun -format UDZO "$TEMP_DMG"
hdiutil verify "$TEMP_DMG"
DIGEST=$(shasum -a 256 "$TEMP_DMG" | awk '{ print $1 }')
printf '%s  %s\n' "$DIGEST" "$DMG" > "$TEMP_SHA"

# Replace the app, image, and checksum as one guarded transaction. A failed
# move or post-move checksum restores the previous verified artifacts.
"$PROJECT_ROOT/scripts/assert-app-idle.sh" "$APP"
if [[ -e "$APP" || -L "$APP" ]]; then mv "$APP" "$PREVIOUS_APP"; fi
if [[ -e "$DMG" || -L "$DMG" ]]; then mv "$DMG" "$PREVIOUS_DMG"; fi
if [[ -e "$SHA" || -L "$SHA" ]]; then mv "$SHA" "$PREVIOUS_SHA"; fi
mv "$STAGED_APP" "$APP"
NEW_APP=1
mv "$TEMP_DMG" "$DMG"
NEW_DMG=1
mv "$TEMP_SHA" "$SHA"
NEW_SHA=1
shasum -a 256 -c "$SHA"
COMMITTED=1
echo "Packaged $DMG"
