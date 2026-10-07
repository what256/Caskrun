#!/bin/sh
# Package a full Vulkan-enabled Wine build as a separate development runtime.
set -eu

if [ "$#" -ne 0 ]; then
  printf 'Usage: %s\n' "$0" >&2
  exit 64
fi

repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
root="$repo/.build/vulkan-candidate-build"
stage="$root/stage"
base="$repo/.build/steam-wine-runtime-dev"
graphics="$repo/.build/graphics-probe"
loader="$graphics/stage/loader-corrected/lib/libvulkan.1.4.350.dylib"
moltenvk="$graphics/upstream/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib"
icd="$graphics/upstream/MoltenVK/MoltenVK/dynamic/dylib/macOS/MoltenVK_icd.json"
target="$repo/.build/vulkan-candidate-runtime"
tmp="$target.tmp.$$"

if [ -e "$target" ] || [ -L "$target" ] || [ -e "$tmp" ]; then
  printf 'Vulkan candidate package already exists; refusing to replace it.\n' >&2
  exit 73
fi
verify() {
  expected=$1 input=$2
  actual=$(/usr/bin/shasum -a 256 "$input" | /usr/bin/awk '{ print $1 }')
  if [ "$actual" != "$expected" ]; then
    printf 'Unexpected SHA-256 for %s\n' "$input" >&2
    exit 65
  fi
}
verify512() {
  expected=$1 input=$2
  actual=$(/usr/bin/shasum -a 512 "$input" | /usr/bin/awk '{ print $1 }')
  if [ "$actual" != "$expected" ]; then
    printf 'Unexpected SHA-512 for %s\n' "$input" >&2
    exit 65
  fi
}
for input in "$stage/bin/wine" \
  "$stage/lib/wine/x86_64-unix/winevulkan.so" \
  "$stage/lib/wine/x86_64-windows/winevulkan.dll" \
  "$stage/lib/wine/i386-windows/winevulkan.dll" \
  "$stage/lib/wine/x86_64-unix/Steam" \
  "$base/lib/libfreetype.6.dylib" "$base/lib/libgnutls.30.dylib" \
  "$base/runtime/share/wine/mono" "$base/runtime/share/wine/gecko" \
  "$base/source/wine-11.0.tar.xz" \
  "$loader" "$moltenvk" "$icd" \
  "$repo/scripts/wine-runtime-vulkan-launch.sh"; do
  if [ ! -e "$input" ]; then
    printf 'Missing Vulkan candidate input: %s\n' "$input" >&2
    exit 66
  fi
done
verify 8f53805f14d208512aa7d00f958ca14ef67751f4e4c43e762d5a363d079784e2 "$loader"
verify aef00b13bcc808adf15b85bef9ae67393d92be7ed5dfe41cad16fa809e4a4c5f "$moltenvk"
verify 578ff08cd0d8734619357541771a5abc9c3470ca300030219a971a9e9dbbe466 "$icd"
verify512 a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b \
  "$base/source/wine-11.0.tar.xz"
verify 408dade743a358886e76b6bb6e592ed6cb8fa67738c1b6d4b061d817c2cbe90f \
  "$base/source/patches/wine-11.0-steam-cef-commandline.patch"
verify 33811970ce63401baabaffd491ce0ac79c69ce740f0ad37687355041f2bd1844 \
  "$base/source/patches/wine-11.0-steam-dock.patch"
verify b28004e117f9ee9d41716bb2d8b6fcda1d03d9f4a72f494706d02c248c17bb70 \
  "$base/source/patches/steam_info.plist.in"
verify f95765a6229cb7b915990a2890ce12ebe36a730b021545d3d52ae69ce4c4024e \
  "$graphics/downloads/MoltenVK-macos-v1.4.2.tar"
case "$(/usr/bin/file -b "$loader")" in
  *Mach-O*x86_64*) ;;
  *) printf 'The Vulkan Loader is not x86_64 Mach-O.\n' >&2; exit 65 ;;
esac
if ! /usr/bin/lipo "$moltenvk" -verify_arch x86_64; then
  printf 'MoltenVK lacks an x86_64 slice.\n' >&2
  exit 65
fi
if ! /usr/bin/grep -q '^#define SONAME_LIBVULKAN "libvulkan.1.dylib"' \
  "$root/build/include/config.h"; then
  printf 'Candidate Wine build lacks Vulkan support.\n' >&2
  exit 65
fi
if [ "$(git -C "$graphics/sources/Vulkan-Loader" rev-parse HEAD)" != a9e72c66d5cb79911eb9a9063bf4016dd0a3a123 ] || \
   [ "$(git -C "$graphics/sources/Vulkan-Headers" rev-parse HEAD)" != 8864cdc896bbc2a9b6eb36b3218fc9ef57908d77 ]; then
  printf 'Vulkan source revisions differ from the reviewed probe.\n' >&2
  exit 65
fi

/bin/mkdir -p "$tmp/bin" "$tmp/lib/vulkan" "$tmp/licenses" "$tmp/source"
trap 'if [ -d "$tmp" ]; then mv "$tmp" "$tmp.incomplete"; fi' EXIT HUP INT TERM
/bin/cp -cR "$stage" "$tmp/runtime"
/bin/cp -cR "$base/runtime/share/wine/mono" "$tmp/runtime/share/wine/"
/bin/cp -cR "$base/runtime/share/wine/gecko" "$tmp/runtime/share/wine/"
/bin/cp -c "$base/lib/libfreetype.6.dylib" "$base/lib/libgnutls.30.dylib" "$tmp/lib/"
/bin/cp -c "$loader" "$tmp/lib/vulkan/libvulkan.1.4.350.dylib"
/bin/ln -s libvulkan.1.4.350.dylib "$tmp/lib/vulkan/libvulkan.1.dylib"
/bin/ln -s libvulkan.1.dylib "$tmp/lib/vulkan/libvulkan.dylib"
/bin/cp -c "$moltenvk" "$tmp/lib/vulkan/libMoltenVK.dylib"
/bin/cp "$icd" "$tmp/lib/vulkan/MoltenVK_icd.json"
/bin/cp "$repo/scripts/wine-runtime-vulkan-launch.sh" "$tmp/bin/_dispatch"
/bin/chmod 755 "$tmp/bin/_dispatch"
/bin/ln -s _dispatch "$tmp/bin/wine"
/bin/ln -s _dispatch "$tmp/bin/wineboot"
/bin/ln -s _dispatch "$tmp/bin/wineserver"
/bin/cp -cR "$base/licenses/." "$tmp/licenses/"
/bin/cp "$graphics/sources/Vulkan-Loader/LICENSE.txt" "$tmp/licenses/Vulkan-Loader-LICENSE.txt"
/bin/cp "$graphics/sources/Vulkan-Headers/LICENSE.md" "$tmp/licenses/Vulkan-Headers-LICENSE.md"
/bin/cp "$graphics/sources/Vulkan-Headers/LICENSES/Apache-2.0.txt" \
  "$tmp/licenses/Vulkan-Headers-Apache-2.0.txt"
/bin/cp "$graphics/sources/Vulkan-Headers/LICENSES/MIT.txt" \
  "$tmp/licenses/Vulkan-Headers-MIT.txt"
/bin/cp "$graphics/upstream/MoltenVK/LICENSE" "$tmp/licenses/MoltenVK-LICENSE.txt"
/bin/cp -cR "$base/source/." "$tmp/source/"
/bin/cp -c "$graphics/downloads/MoltenVK-macos-v1.4.2.tar" "$tmp/source/"

# Sign copies only. The original stage and active package are never modified.
for input in "$tmp/lib/vulkan/libvulkan.1.4.350.dylib" \
  "$tmp/lib/vulkan/libMoltenVK.dylib"; do
  /usr/bin/codesign --force --sign - "$input"
  /usr/bin/codesign --verify --strict "$input"
done
find "$tmp/runtime/bin" "$tmp/runtime/lib/wine/x86_64-unix" -type f -print | while IFS= read -r input; do
  case "$(/usr/bin/file -b "$input")" in
    *Mach-O*x86_64*)
      /usr/bin/codesign --force --sign - "$input"
      /usr/bin/codesign --verify --strict "$input"
      ;;
    *Mach-O*) printf 'Wrong Unix Mach-O architecture: %s\n' "$input" >&2; exit 65 ;;
  esac
done

find "$tmp/runtime/bin" "$tmp/runtime/lib/wine/x86_64-unix" "$tmp/lib" -type f -print | while IFS= read -r input; do
  case "$(/usr/bin/file -b "$input")" in
    *Mach-O*)
      /usr/bin/otool -arch x86_64 -L "$input" | /usr/bin/awk 'NR > 1 { print $1 }' | while IFS= read -r dep; do
        case "$dep" in
          /usr/lib/*|/System/Library/*|@rpath/*|@loader_path/*|@executable_path/*) ;;
          *) printf 'External Mach-O dependency in %s: %s\n' "$input" "$dep" >&2; exit 65 ;;
        esac
      done
      ;;
  esac
done
find "$tmp" -type l -print | while IFS= read -r link; do
  case "$(/usr/bin/readlink "$link")" in
    /*) printf 'Absolute package symlink: %s\n' "$link" >&2; exit 65 ;;
  esac
done

cat > "$tmp/SOURCES.txt" <<'EOF'
Development-only Vulkan-enabled Wine runtime candidate; not a notarized release.
Wine 11.0 source and original Caskrun Steam patches: source/ and source/SOURCES.txt.
Wine, GnuTLS, FreeType, Mono, Gecko notices: licenses/ and the independent
Steam development runtime's SOURCES.txt; all inputs are independently sourced.
Vulkan Loader v1.4.350: https://github.com/KhronosGroup/Vulkan-Loader
Commit: a9e72c66d5cb79911eb9a9063bf4016dd0a3a123
Built x86_64 loader SHA-256 (before package signing): 8f53805f14d208512aa7d00f958ca14ef67751f4e4c43e762d5a363d079784e2
Vulkan Headers v1.4.350: https://github.com/KhronosGroup/Vulkan-Headers
Commit: 8864cdc896bbc2a9b6eb36b3218fc9ef57908d77
MoltenVK 1.4.2 macOS release: https://github.com/KhronosGroup/MoltenVK/releases/tag/v1.4.2
Official archive SHA-256: f95765a6229cb7b915990a2890ce12ebe36a730b021545d3d52ae69ce4c4024e
Original universal dylib SHA-256: aef00b13bcc808adf15b85bef9ae67393d92be7ed5dfe41cad16fa809e4a4c5f
Original ICD JSON SHA-256: 578ff08cd0d8734619357541771a5abc9c3470ca300030219a971a9e9dbbe466
Loader and MoltenVK copies are ad-hoc signed; checksums change with signing.
Vulkan Loader: Apache-2.0 with permissive exceptions; Headers: Apache-2.0/MIT;
MoltenVK: Apache-2.0. See licenses/ for complete upstream texts.
The exact official MoltenVK release tar is retained under source/ for this
development candidate. Direct3D game compatibility is not established.
EOF

version=$(/usr/bin/env \
  -u DYLD_LIBRARY_PATH -u DYLD_FALLBACK_LIBRARY_PATH \
  -u VK_DRIVER_FILES -u VK_ICD_FILENAMES \
  -u WINEDLLPATH -u WINEDATADIR -u WINELOADER -u WINESERVER \
  "$tmp/bin/wine" --version)
if [ "$version" != wine-11.0 ]; then
  printf 'Packaged Vulkan Wine did not report wine-11.0: %s\n' "$version" >&2
  exit 65
fi

/bin/mv "$tmp" "$target"
trap - EXIT HUP INT TERM
printf 'Isolated Vulkan candidate package: %s\n' "$target"
