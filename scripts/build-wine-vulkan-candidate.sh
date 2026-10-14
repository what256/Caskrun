#!/bin/sh
# Build a completely separate Wine 11.0 tree with its Vulkan Unix modules.
# This never changes the Wine package used by the current Caskrun/Steam run.
set -eu

if [ "$#" -ne 0 ]; then
  printf 'Usage: %s\n' "$0" >&2
  exit 64
fi

repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
root="$repo/.build/vulkan-candidate-build"
archive="$repo/.build/wine-build-probe/wine-11.0.tar.xz"
tls="$repo/.build/tls-probe/prefix"
freetype="$repo/.build/wine-build-probe/deps-x86_64"
bison="$repo/.build/wine-build-probe/toolchain/bin/bison"
headers="$repo/.build/graphics-probe/stage/headers"
loader="$repo/.build/graphics-probe/stage/loader-corrected"
steam_patch="$repo/runtime-patches/wine-11.0-steam-cef-commandline.patch"
dock_patch="$repo/runtime-patches/wine-11.0-steam-dock.patch"
steam_plist="$repo/runtime-patches/steam_info.plist.in"

verify() {
  algorithm=$1 expected=$2 input=$3
  actual=$(/usr/bin/shasum -a "$algorithm" "$input" | /usr/bin/awk '{ print $1 }')
  if [ "$actual" != "$expected" ]; then
    printf 'Unexpected SHA-%s for %s\n' "$algorithm" "$input" >&2
    exit 65
  fi
}

if [ -e "$root" ] || [ -L "$root" ]; then
  printf 'Vulkan candidate build already exists: %s\n' "$root" >&2
  exit 73
fi
for input in "$archive" "$tls/lib/libgnutls.30.dylib" \
  "$freetype/lib/libfreetype.6.dylib" "$bison" \
  "$headers/include/vulkan/vulkan.h" "$loader/lib/libvulkan.1.4.350.dylib" \
  "$steam_patch" "$dock_patch" "$steam_plist"; do
  if [ ! -f "$input" ]; then
    printf 'Missing independent build input: %s\n' "$input" >&2
    exit 66
  fi
done
verify 512 a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b "$archive"
verify 256 8f53805f14d208512aa7d00f958ca14ef67751f4e4c43e762d5a363d079784e2 "$loader/lib/libvulkan.1.4.350.dylib"
verify 256 408dade743a358886e76b6bb6e592ed6cb8fa67738c1b6d4b061d817c2cbe90f "$steam_patch"
verify 256 33811970ce63401baabaffd491ce0ac79c69ce740f0ad37687355041f2bd1844 "$dock_patch"
verify 256 60ce853901a6297629fdb7681b743c8338b5dd21875b6f69182c1dbf2b427d9c "$steam_plist"

if [ "$(/usr/bin/uname -m)" != arm64 ] || \
   [ "$(/usr/bin/arch -x86_64 /usr/bin/uname -m)" != x86_64 ]; then
  printf 'Apple Silicon and Rosetta are required.\n' >&2
  exit 65
fi
if [ "$(git -C "$repo/.build/graphics-probe/sources/Vulkan-Loader" rev-parse HEAD)" != a9e72c66d5cb79911eb9a9063bf4016dd0a3a123 ] || \
   [ "$(git -C "$repo/.build/graphics-probe/sources/Vulkan-Headers" rev-parse HEAD)" != 8864cdc896bbc2a9b6eb36b3218fc9ef57908d77 ]; then
  printf 'Vulkan source revisions differ from the reviewed probe.\n' >&2
  exit 65
fi

/bin/mkdir -p "$root/src" "$root/build" "$root/stage"
/usr/bin/tar -xJf "$archive" -C "$root/src"
source="$root/src/wine-11.0"
stage="$root/stage"
cd "$source"
/usr/bin/patch --batch -p1 -F 0 < "$steam_patch" > "$root/patch.log" 2>&1
/usr/bin/patch --batch -p1 -F 0 < "$dock_patch" >> "$root/patch.log" 2>&1

cd "$root/build"
env -u PKG_CONFIG_PATH \
  PKG_CONFIG_LIBDIR="$tls/lib/pkgconfig:$freetype/lib/pkgconfig" \
  CC='/usr/bin/clang -target x86_64-apple-macos13' \
  CXX='/usr/bin/clang++ -target x86_64-apple-macos13' \
  BISON="$bison" \
  CPPFLAGS="-I$headers/include" \
  LDFLAGS="-L$loader/lib" \
  /usr/bin/arch -x86_64 /bin/sh "$source/configure" \
    --build=x86_64-apple-darwin \
    --enable-archs=i386,x86_64 \
    --disable-tests \
    --without-x \
    --without-gstreamer \
    --with-gnutls \
    --with-vulkan \
    --prefix="$stage" \
    > configure.log 2>&1

/usr/bin/grep -q '^#define SONAME_LIBVULKAN "libvulkan.1.dylib"' include/config.h
/usr/bin/grep -q '^#define HAVE_GNUTLS_CIPHER_INIT 1' include/config.h
/usr/bin/make -j8 > make.log 2>&1
/usr/bin/make install > make-install.log 2>&1

# Retain Steam's existing loader behavior in the Vulkan candidate. The native
# alias is linked from this exact build and never copied from the active one.
/usr/bin/sed 's/@PACKAGE_VERSION@/11.0/g' "$steam_plist" \
  > "$root/build/loader/steam_info.plist"
/usr/bin/plutil -lint "$root/build/loader/steam_info.plist"
/usr/bin/clang -target x86_64-apple-macos13 -o "$root/build/loader/Steam" \
  "$root/build/loader/main.o" \
  -Wl,-segalign,0x1000,-pagezero_size,0x1000,-sectcreate,__TEXT,__info_plist,"$root/build/loader/steam_info.plist" \
  -Wl,-no_pie,-image_base,0x200000000,-no_huge,-no_fixup_chains,-segalign,0x1000,-segaddr,WINE_RESERVE,0x1000,-segaddr,WINE_TOP_DOWN,0x7ff000000000 \
  > "$root/build/steam-loader-link.log" 2>&1
/usr/bin/codesign --force --sign - "$root/build/loader/Steam"
/bin/cp "$root/build/loader/Steam" "$stage/lib/wine/x86_64-unix/Steam"

for input in "$stage/bin/wine" \
  "$stage/lib/wine/x86_64-unix/winevulkan.so" \
  "$stage/lib/wine/x86_64-windows/winevulkan.dll" \
  "$stage/lib/wine/i386-windows/winevulkan.dll" \
  "$stage/lib/wine/x86_64-unix/Steam"; do
  if [ ! -f "$input" ]; then
    printf 'Missing built Vulkan/Steam component: %s\n' "$input" >&2
    exit 65
  fi
done
case "$(/usr/bin/file -b "$stage/bin/wine")" in
  *Mach-O*x86_64*) ;;
  *) printf 'Wine loader is not x86_64 Mach-O.\n' >&2; exit 65 ;;
esac
case "$(/usr/bin/file -b "$stage/lib/wine/x86_64-unix/winevulkan.so")" in
  *Mach-O*x86_64*) ;;
  *) printf 'Wine Vulkan Unix module is not x86_64 Mach-O.\n' >&2; exit 65 ;;
esac
case "$(/usr/bin/file -b "$stage/lib/wine/x86_64-windows/winevulkan.dll")" in
  *PE32+*x86-64*) ;;
  *) printf 'Wine Vulkan x64 DLL has an unexpected architecture.\n' >&2; exit 65 ;;
esac
case "$(/usr/bin/file -b "$stage/lib/wine/i386-windows/winevulkan.dll")" in
  *PE32*Intel\ 80386*) ;;
  *) printf 'Wine Vulkan x86 DLL has an unexpected architecture.\n' >&2; exit 65 ;;
esac

printf 'Isolated Vulkan Wine stage: %s\n' "$stage"
