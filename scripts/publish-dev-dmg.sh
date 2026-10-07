#!/bin/zsh
set -euo pipefail

PROJECT_ROOT="${0:A:h:h}"
BUILD_ROOT="$PROJECT_ROOT/.build"
SOURCE_DIR="${1:-$BUILD_ROOT/release-candidate}"
SOURCE_DIR="$(cd -- "$SOURCE_DIR" && pwd -P)"
if [[ "$SOURCE_DIR" != "$BUILD_ROOT/"* ]]; then
  print -u2 "The development package must come from inside $BUILD_ROOT"
  exit 64
fi

APP="$SOURCE_DIR/Caskrun.app"
if [[ -L "$APP" || ! -d "$APP" || ! -f "$APP/Contents/Resources/.caskrun-generated-app" ||
      ! -x "$APP/Contents/MacOS/Caskrun" || ! -x "$APP/Contents/MacOS/caskrun-core" ]]; then
  print -u2 "No recognized Caskrun development app exists at $APP"
  exit 66
fi
IDENTIFIER=$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$APP/Contents/Info.plist")
VERSION=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$APP/Contents/Info.plist")
if [[ "$IDENTIFIER" != dev.caskrun.app || "$VERSION" != <->.<->.<-> ]]; then
  print -u2 "The development app has an unexpected identity or version."
  exit 65
fi
NAME="Caskrun-$VERSION-arm64-dev.dmg"
SOURCE_DMG="$SOURCE_DIR/$NAME"
SOURCE_SHA="$SOURCE_DMG.sha256"
if [[ -L "$SOURCE_DMG" || -L "$SOURCE_SHA" || ! -f "$SOURCE_DMG" || ! -f "$SOURCE_SHA" ]]; then
  print -u2 "The development DMG and its checksum are missing or unsafe."
  exit 66
fi
codesign --verify --deep --strict "$APP"
shasum -a 256 -c "$SOURCE_SHA"
hdiutil verify "$SOURCE_DMG" >/dev/null

DIST="$PROJECT_ROOT/dist"
mkdir -p "$DIST"
DEST_DMG="$DIST/$NAME"
DEST_SHA="$DEST_DMG.sha256"
if [[ -e "$DEST_DMG" || -e "$DEST_SHA" || -L "$DEST_DMG" || -L "$DEST_SHA" ]]; then
  if [[ -L "$DEST_DMG" || -L "$DEST_SHA" || ! -f "$DEST_DMG" || ! -f "$DEST_SHA" ]] ||
      ! (cd "$DIST" && shasum -a 256 -c "$NAME.sha256" >/dev/null); then
    print -u2 "Refusing to replace an unverified deliverable in $DIST"
    exit 65
  fi
fi

WORK="$(mktemp -d "$DIST/.publish.XXXXXXXX")"
mkdir "$WORK/new" "$WORK/previous" "$WORK/failed"
OLD_DMG_MOVED=0
OLD_SHA_MOVED=0
NEW_DMG_MOVED=0
NEW_SHA_MOVED=0
PUBLISHED=0

cleanup() {
  if (( ! PUBLISHED )); then
    if (( NEW_SHA_MOVED )) && [[ -e "$DEST_SHA" ]]; then
      mv "$DEST_SHA" "$WORK/failed/$NAME.sha256" || true
    fi
    if (( NEW_DMG_MOVED )) && [[ -e "$DEST_DMG" ]]; then
      mv "$DEST_DMG" "$WORK/failed/$NAME" || true
    fi
    if (( OLD_SHA_MOVED )) && [[ -e "$WORK/previous/$NAME.sha256" ]]; then
      mv "$WORK/previous/$NAME.sha256" "$DEST_SHA" || true
    fi
    if (( OLD_DMG_MOVED )) && [[ -e "$WORK/previous/$NAME" ]]; then
      mv "$WORK/previous/$NAME" "$DEST_DMG" || true
    fi
    print -u2 "Publication failed; staged files remain at $WORK"
  else
    rm -r "$WORK"
  fi
}
trap cleanup EXIT

cp "$SOURCE_DMG" "$WORK/new/$NAME"
SOURCE_DIGEST=$(shasum -a 256 "$SOURCE_DMG" | awk '{ print $1 }')
STAGED_DIGEST=$(shasum -a 256 "$WORK/new/$NAME" | awk '{ print $1 }')
if [[ "$SOURCE_DIGEST" != "$STAGED_DIGEST" ]]; then
  print -u2 "The copied development DMG changed during staging."
  exit 65
fi
printf '%s  %s\n' "$SOURCE_DIGEST" "$NAME" > "$WORK/new/$NAME.sha256"
(cd "$WORK/new" && shasum -a 256 -c "$NAME.sha256")

if [[ -e "$DEST_DMG" ]]; then
  mv "$DEST_DMG" "$WORK/previous/$NAME"
  OLD_DMG_MOVED=1
  mv "$DEST_SHA" "$WORK/previous/$NAME.sha256"
  OLD_SHA_MOVED=1
fi
mv "$WORK/new/$NAME" "$DEST_DMG"
NEW_DMG_MOVED=1
mv "$WORK/new/$NAME.sha256" "$DEST_SHA"
NEW_SHA_MOVED=1
(cd "$DIST" && shasum -a 256 -c "$NAME.sha256")
PUBLISHED=1
echo "Published $DEST_DMG"
