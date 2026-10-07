#!/bin/sh
# Package the already-built, project-local Wine experiment for relocation tests.
# This creates a development artifact only; it is not included in the app DMG.
set -eu

if [ "$#" -ne 0 ]; then
  printf 'Usage: %s\n' "$0" >&2
  exit 64
fi

repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
probe=$repo/.build/wine-build-probe
stage=$probe/stage
freetype=$probe/deps-x86_64/lib/libfreetype.6.dylib
target=$repo/.build/caskrun-wine-runtime-dev
wine_source=$probe/src/wine-11.0
freetype_source=$probe/dependency-src/freetype-2.14.3
addon_inputs=$repo/.build/mono-gecko-probe
addon_downloads=$addon_inputs/downloads
addon_licenses=$addon_inputs/licenses

if [ "$(uname -m)" != arm64 ]; then
  printf 'This development runtime targets Apple Silicon Macs.\n' >&2
  exit 65
fi
if [ -e "$target" ] || [ -L "$target" ]; then
  printf 'Package already exists; move it aside before rebuilding: %s\n' "$target" >&2
  exit 73
fi
for required in "$stage/bin/wine" "$stage/bin/wineboot" "$stage/bin/wineserver" \
  "$freetype" "$wine_source/LICENSE" "$wine_source/COPYING.LIB" \
  "$freetype_source/docs/FTL.TXT" "$freetype_source/docs/GPLv2.TXT" \
  "$addon_downloads/wine-mono-10.4.1-x86.msi" \
  "$addon_downloads/wine-gecko-2.47.4-x86.msi" \
  "$addon_downloads/wine-gecko-2.47.4-x86_64.msi" \
  "$addon_licenses/Mono-COPYING.txt" \
  "$addon_licenses/Gecko-LICENSE.txt" \
  "$addon_licenses/Gecko-license.html"; do
  if [ ! -f "$required" ]; then
    printf 'Missing local build input: %s\n' "$required" >&2
    exit 66
  fi
done

verify_sha256() {
  expected=$1
  input=$2
  actual=$(/usr/bin/shasum -a 256 "$input" | /usr/bin/awk '{ print $1 }')
  if [ "$actual" != "$expected" ]; then
    printf 'SHA-256 mismatch for %s: expected %s, got %s\n' "$input" "$expected" "$actual" >&2
    exit 65
  fi
}

# Exact Wine 11.0 addon versions and MSI hashes come from its
# dlls/appwiz.cpl/addons.c. Local MSIs are not hashed by Wine's file lookup,
# so verify before placing them in WINEDATADIR.
verify_sha256 071f4b2887e1c97a11d791ff3d65be9429eed6dec4c2708888bfd546ba358e23 \
  "$addon_downloads/wine-mono-10.4.1-x86.msi"
verify_sha256 26cecc47706b091908f7f814bddb074c61beb8063318e9efc5a7f789857793d6 \
  "$addon_downloads/wine-gecko-2.47.4-x86.msi"
verify_sha256 e590b7d988a32d6aa4cf1d8aa3aa3d33766fdd4cf4c89c2dcc2095ecb28d066f \
  "$addon_downloads/wine-gecko-2.47.4-x86_64.msi"
verify_sha256 0265443ffc97c9450dbcf402c2430bf9fe87d7a6c92d5d7684fcecfe749bf75d \
  "$addon_licenses/Mono-COPYING.txt"
verify_sha256 a799adf18bfb1911f16086dc0e2a6d1cd0e31272f47e7ee6841b465b74d249de \
  "$addon_licenses/Gecko-LICENSE.txt"
verify_sha256 18412e6665fd12c854c85aef601a564bda44363845828b158e7e93e0c2d8f177 \
  "$addon_licenses/Gecko-license.html"

if ! /usr/bin/file "$stage/bin/wine" "$stage/bin/wineserver" "$freetype" | /usr/bin/awk '
  /Mach-O 64-bit/ && /x86_64/ { good++ }
  END { exit good == 3 ? 0 : 1 }
'; then
  printf 'Expected x86_64 Mach-O Wine and FreeType inputs.\n' >&2
  exit 65
fi

/usr/bin/codesign --verify --strict "$stage/bin/wine" "$stage/bin/wineserver" "$freetype"

# Reject a staged build that escaped into an absolute symlink or a foreign
# direct Mach-O dependency before we clone it into the package.
find "$stage" -type l -print | while IFS= read -r link; do
  case "$(readlink "$link")" in
    /*) printf 'Absolute staged symlink: %s\n' "$link" >&2; exit 65 ;;
  esac
done
find "$stage/bin" "$stage/lib/wine/x86_64-unix" -type f -print | while IFS= read -r file; do
  description=$(/usr/bin/file -b "$file")
  case "$description" in
    *Mach-O*)
      case "$description" in
        *x86_64*) ;;
        *) printf 'Non-x86_64 Wine Mach-O file: %s\n' "$file" >&2; exit 65 ;;
      esac
      /usr/bin/codesign --verify --strict "$file"
      /usr/bin/otool -L "$file" | /usr/bin/awk 'NR > 1 { print $1 }' | while IFS= read -r dependency; do
        case "$dependency" in
          /usr/lib/*|/System/Library/*|@rpath/*|@loader_path/*|@executable_path/*) ;;
          *) printf 'External Mach-O dependency in %s: %s\n' "$file" "$dependency" >&2; exit 65 ;;
        esac
      done
      ;;
  esac
done

tmp=$repo/.build/.caskrun-wine-runtime-dev.$$
if [ -e "$tmp" ] || [ -L "$tmp" ]; then
  printf 'Temporary path already exists: %s\n' "$tmp" >&2
  exit 73
fi
mkdir -p "$tmp/bin" "$tmp/lib" "$tmp/licenses"
trap 'if [ -d "$tmp" ]; then mv "$tmp" "$tmp.incomplete"; fi' EXIT HUP INT TERM

# APFS clone copy avoids a second physical 1.5 GB allocation where supported.
/bin/cp -cR "$stage" "$tmp/runtime"
/bin/mkdir -p "$tmp/runtime/share/wine/mono" "$tmp/runtime/share/wine/gecko"
/bin/cp -c "$addon_downloads/wine-mono-10.4.1-x86.msi" "$tmp/runtime/share/wine/mono/"
/bin/cp -c "$addon_downloads/wine-gecko-2.47.4-x86.msi" \
  "$addon_downloads/wine-gecko-2.47.4-x86_64.msi" "$tmp/runtime/share/wine/gecko/"
/bin/cp -c "$freetype" "$tmp/lib/libfreetype.6.dylib"
# The locally built FreeType dylib advertises an absolute build-tree install
# name. Give the packaged copy a relative identity and re-sign that copy.
/usr/bin/install_name_tool -id '@rpath/libfreetype.6.dylib' "$tmp/lib/libfreetype.6.dylib"
/usr/bin/codesign --force --sign - "$tmp/lib/libfreetype.6.dylib"
/bin/cp "$repo/scripts/wine-runtime-launch.sh" "$tmp/bin/_dispatch"
chmod 755 "$tmp/bin/_dispatch"
ln -s _dispatch "$tmp/bin/wine"
ln -s _dispatch "$tmp/bin/wineboot"
ln -s _dispatch "$tmp/bin/wineserver"

/bin/cp "$wine_source/LICENSE" "$tmp/licenses/Wine-LICENSE.txt"
/bin/cp "$wine_source/COPYING.LIB" "$tmp/licenses/Wine-LGPL-2.1.txt"
/bin/cp "$freetype_source/docs/FTL.TXT" "$tmp/licenses/FreeType-FTL.txt"
/bin/cp "$freetype_source/docs/GPLv2.TXT" "$tmp/licenses/FreeType-GPLv2.txt"
/bin/cp "$addon_licenses/Mono-COPYING.txt" "$tmp/licenses/Wine-Mono-COPYING.txt"
/bin/cp "$addon_licenses/Gecko-LICENSE.txt" "$tmp/licenses/Wine-Gecko-LICENSE.txt"
/bin/cp "$addon_licenses/Gecko-license.html" "$tmp/licenses/Wine-Gecko-license.html"
cat > "$tmp/SOURCES.txt" <<'EOF'
Development-only Caskrun Wine runtime (not a release or a notarized app component).

Wine 11.0: https://dl.winehq.org/wine/source/11.0/wine-11.0.tar.xz
SHA-512: a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b
License: LGPL-2.1-or-later; see licenses/Wine-LICENSE.txt and licenses/Wine-LGPL-2.1.txt.

FreeType 2.14.3: https://download.savannah.gnu.org/releases/freetype/freetype-2.14.3.tar.xz
SHA-256: 36bc4f1cc413335368ee656c42afca65c5a3987e8768cc28cf11ba775e785a5f
License: FreeType License or GPL-2.0; see licenses/FreeType-FTL.txt and licenses/FreeType-GPLv2.txt.

Wine Mono 10.4.1 x86 MSI: https://dl.winehq.org/wine/wine-mono/10.4.1/wine-mono-10.4.1-x86.msi
SHA-256: 071f4b2887e1c97a11d791ff3d65be9429eed6dec4c2708888bfd546ba358e23
Matching source and mixed license notice: https://github.com/wine-mono/wine-mono/tree/wine-mono-10.4.1
See licenses/Wine-Mono-COPYING.txt. Third-party component terms need a release audit.

Wine Gecko 2.47.4 x86 MSI: https://dl.winehq.org/wine/wine-gecko/2.47.4/wine-gecko-2.47.4-x86.msi
SHA-256: 26cecc47706b091908f7f814bddb074c61beb8063318e9efc5a7f789857793d6
Wine Gecko 2.47.4 x86_64 MSI: https://dl.winehq.org/wine/wine-gecko/2.47.4/wine-gecko-2.47.4-x86_64.msi
SHA-256: e590b7d988a32d6aa4cf1d8aa3aa3d33766fdd4cf4c89c2dcc2095ecb28d066f
Matching source: https://gitlab.winehq.org/wine/wine-gecko/-/tree/wine-gecko-2.47.4
See licenses/Wine-Gecko-LICENSE.txt and licenses/Wine-Gecko-license.html
for MPL-2.0 and bundled third-party terms. A release needs full notice review.

The corresponding source archives and project-local build logs are retained in
this repository's .build/wine-build-probe/ during development. A redistributed build needs a
documented source-availability, dependency, signing, and notarization policy.
EOF

/usr/bin/codesign --verify --strict "$tmp/runtime/bin/wine" "$tmp/runtime/bin/wineserver" "$tmp/lib/libfreetype.6.dylib"
mv "$tmp" "$target"
trap - EXIT HUP INT TERM
printf '%s\n' "$target/bin/wine"
