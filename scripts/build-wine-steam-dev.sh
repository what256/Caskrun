#!/bin/sh
# Build the reviewed Steam-specific Wine changes from fresh WineHQ source.
# Development output only: never replaces the current Caskrun app or runtime.
set -eu

case "${1-}" in
  '') mode=build ;;
  --check) mode=check ;;
  *) printf 'Usage: %s [--check]\n' "$0" >&2; exit 64 ;;
esac
if [ "$#" -gt 1 ]; then
  printf 'Usage: %s [--check]\n' "$0" >&2
  exit 64
fi

repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
archive="$repo/.build/wine-build-probe/wine-11.0.tar.xz"
wine_sha512=a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b
tls="$repo/.build/tls-probe/prefix"
freetype="$repo/.build/wine-build-probe/deps-x86_64"
bison="$repo/.build/wine-build-probe/toolchain/bin/bison"

# Fixed allowlist: do not accept arbitrary patch paths from the environment or
# command line. Each change must be separately reviewed and checksum-pinned.
steam_patch="$repo/runtime-patches/wine-11.0-steam-cef-commandline.patch"
steam_patch_sha256=408dade743a358886e76b6bb6e592ed6cb8fa67738c1b6d4b061d817c2cbe90f
dock_patch="$repo/runtime-patches/wine-11.0-steam-dock.patch"
dock_patch_sha256=33811970ce63401baabaffd491ce0ac79c69ce740f0ad37687355041f2bd1844
steam_plist="$repo/runtime-patches/steam_info.plist.in"
steam_plist_sha256=60ce853901a6297629fdb7681b743c8338b5dd21875b6f69182c1dbf2b427d9c
output="$repo/.build/wine-steam-11.0-cef-dock-dev"

verify_hash() {
  algorithm=$1
  expected=$2
  input=$3
  actual=$(/usr/bin/shasum -a "$algorithm" "$input" | /usr/bin/awk '{ print $1 }')
  if [ "$actual" != "$expected" ]; then
    printf 'SHA-%s mismatch: %s\n' "$algorithm" "$input" >&2
    exit 65
  fi
}

for input in "$archive" "$steam_patch" "$dock_patch" "$steam_plist" \
  "$tls/lib/pkgconfig/gnutls.pc" \
  "$tls/lib/libgnutls.30.dylib" "$freetype/lib/pkgconfig/freetype2.pc" \
  "$freetype/lib/libfreetype.6.dylib" "$bison"; do
  if [ ! -f "$input" ]; then
    printf 'Missing independent build input: %s\n' "$input" >&2
    exit 66
  fi
done

if [ "$(/usr/bin/uname -m)" != arm64 ] || \
   [ "$(/usr/bin/arch -x86_64 /usr/bin/uname -m)" != x86_64 ]; then
  printf 'This build needs Apple Silicon and Rosetta.\n' >&2
  exit 65
fi
if ! /usr/bin/file "$tls/lib/libgnutls.30.dylib" \
  "$freetype/lib/libfreetype.6.dylib" | \
  /usr/bin/awk '/Mach-O/ && /x86_64/ { found++ } END { exit found == 2 ? 0 : 1 }'; then
  printf 'Expected project-local x86_64 GnuTLS and FreeType inputs.\n' >&2
  exit 65
fi
verify_hash 512 "$wine_sha512" "$archive"
verify_hash 256 "$steam_patch_sha256" "$steam_patch"
verify_hash 256 "$dock_patch_sha256" "$dock_patch"
verify_hash 256 "$steam_plist_sha256" "$steam_plist"

if [ "$mode" = check ]; then
  # Test every pinned change against a fresh extraction of exact upstream
  # Wine. The scratch tree is removed and the stable runtime is untouched.
  scratch=$(/usr/bin/mktemp -d "$repo/.build/.wine-steam-patch-check.XXXXXX")
  trap 'rm -rf "$scratch"' EXIT HUP INT TERM
  /usr/bin/tar -xJf "$archive" -C "$scratch"
  cd "$scratch/wine-11.0"
  /usr/bin/patch --batch --dry-run -p1 -F 0 < "$steam_patch"
  /usr/bin/patch --batch --dry-run -p1 -F 0 < "$dock_patch"
  /usr/bin/sed 's/@PACKAGE_VERSION@/11.0/g' "$steam_plist" \
    > "$scratch/steam_info.plist"
  /usr/bin/plutil -lint "$scratch/steam_info.plist"
  printf 'Verified WineHQ Wine 11.0 archive, both reviewed patches, and Steam loader plist.\n'
  exit 0
fi

if [ -e "$output" ] || [ -L "$output" ]; then
  printf 'Isolated Wine build already exists; refusing to replace it: %s\n' "$output" >&2
  exit 73
fi

/bin/mkdir -p "$output/src" "$output/build" "$output/stage"
/usr/bin/tar -xJf "$archive" -C "$output/src"
source="$output/src/wine-11.0"
build="$output/build"
stage="$output/stage"
cd "$source"
/usr/bin/patch --batch -p1 -F 0 < "$steam_patch" \
  > "$output/patch.log" 2>&1
/usr/bin/patch --batch -p1 -F 0 < "$dock_patch" \
  >> "$output/patch.log" 2>&1

cd "$build"
env -u PKG_CONFIG_PATH \
  PKG_CONFIG_LIBDIR="$tls/lib/pkgconfig:$freetype/lib/pkgconfig" \
  CC='/usr/bin/clang -target x86_64-apple-macos13' \
  CXX='/usr/bin/clang++ -target x86_64-apple-macos13' \
  BISON="$bison" \
  /usr/bin/arch -x86_64 /bin/sh "$source/configure" \
    --build=x86_64-apple-darwin \
    --enable-archs=i386,x86_64 \
    --disable-tests \
    --without-x \
    --without-gstreamer \
    --with-gnutls \
    --prefix="$stage" \
    > configure.log 2>&1

/usr/bin/grep -q '^#define SONAME_LIBGNUTLS' include/config.h
/usr/bin/grep -q '^#define HAVE_GNUTLS_CIPHER_INIT 1' include/config.h
/usr/bin/make -j8 > make.log 2>&1
/usr/bin/make install > make-install.log 2>&1

# Wine's loader rule embeds a generic Wine plist. Relink the same built object
# with the reviewed Steam plist so macOS gives matching Steam processes their
# own app identity. The source patch selects this alias only for Steam and its
# web helper while the launch opt-in is set.
/usr/bin/sed 's/@PACKAGE_VERSION@/11.0/g' "$steam_plist" \
  > "$build/loader/steam_info.plist"
/usr/bin/plutil -lint "$build/loader/steam_info.plist"
cd "$build"
/usr/bin/clang -target x86_64-apple-macos13 -o loader/Steam loader/main.o \
  -Wl,-segalign,0x1000,-pagezero_size,0x1000,-sectcreate,__TEXT,__info_plist,loader/steam_info.plist \
  -Wl,-no_pie,-image_base,0x200000000,-no_huge,-no_fixup_chains,-segalign,0x1000,-segaddr,WINE_RESERVE,0x1000,-segaddr,WINE_TOP_DOWN,0x7ff000000000 \
  > steam-loader-link.log 2>&1
/usr/bin/codesign --force --sign - loader/Steam
/usr/bin/codesign --verify --strict loader/Steam
/bin/cp loader/Steam "$stage/lib/wine/x86_64-unix/Steam"
/usr/bin/codesign --verify --strict "$stage/lib/wine/x86_64-unix/Steam"

if ! /usr/bin/file "$stage/bin/wine" "$stage/lib/wine/x86_64-unix/ntdll.so" \
  "$stage/lib/wine/x86_64-unix/Steam" | \
  /usr/bin/awk '/Mach-O/ && /x86_64/ { found++ } END { exit found == 3 ? 0 : 1 }'; then
  printf 'Expected x86_64 Wine loader, patched NTDLL, and Steam alias.\n' >&2
  exit 65
fi

# Keep the exact corresponding Wine source and our change with this stage.
# A later packager must carry these files, the Wine LGPL notices, and the
# dependency notices forward into any distributed runtime.
/bin/mkdir -p "$stage/caskrun-source/patches" "$stage/caskrun-source/licenses"
/bin/cp "$archive" "$stage/caskrun-source/wine-11.0.tar.xz"
/bin/cp "$steam_patch" "$dock_patch" "$steam_plist" \
  "$stage/caskrun-source/patches/"
/bin/cp "$source/LICENSE" "$stage/caskrun-source/licenses/Wine-LICENSE.txt"
/bin/cp "$source/COPYING.LIB" "$stage/caskrun-source/licenses/Wine-LGPL-2.1.txt"
cat > "$stage/caskrun-source/SOURCES.txt" <<EOF
Development-only Caskrun Steam Wine build; not a release or notarized package.
Wine 11.0 source: https://dl.winehq.org/wine/source/11.0/wine-11.0.tar.xz
Wine source SHA-512: $wine_sha512
Wine license: LGPL-2.1-or-later; see licenses/ and the included source archive.
Original Caskrun patches: patches/wine-11.0-steam-cef-commandline.patch and patches/wine-11.0-steam-dock.patch
CEF patch SHA-256: $steam_patch_sha256
Dock patch SHA-256: $dock_patch_sha256
Steam loader plist template: patches/steam_info.plist.in
Loader plist template SHA-256: $steam_plist_sha256
Patch and loader template license: LGPL-2.1-or-later; see runtime-patches/README.md in the project.
Build configuration: x86_64 macOS 13 target, i386+x86_64 PE, GnuTLS enabled.
Steam identity alias: lib/wine/x86_64-unix/Steam, ad-hoc signed.
The verified Steam launch sets CASKRUN_STEAM_MODE=1 and an original Steam.exe-derived CASKRUN_STEAM_ICON .ico path.
GnuTLS, FreeType, and other component source/notice details: docs/LOCAL_WINE_RUNTIME.md.
EOF

printf 'Isolated patched Wine stage: %s\n' "$stage"
