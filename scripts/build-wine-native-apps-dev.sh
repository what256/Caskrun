#!/bin/sh
# Rebuild original native-identity changes in an APFS clone of the reviewed
# Steam Wine build. Never modifies the seed tree or an installed application.
set -eu
case "${1-}" in
  '') mode=build ;;
  --check) mode=check ;;
  *) printf 'Usage: %s [--check]\n' "$0" >&2; exit 64 ;;
esac
[ "$#" -le 1 ] || exit 64
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
seed="$repo/.build/wine-steam-11.0-cef-dock-dev"
output="$repo/.build/wine-native-apps-11.0-dev"
archive="$repo/.build/wine-build-probe/wine-11.0.tar.xz"
steam_patch="$repo/runtime-patches/wine-11.0-steam-cef-commandline.patch"
dock_patch="$repo/runtime-patches/wine-11.0-steam-dock.patch"
native_patch="$repo/runtime-patches/wine-11.0-native-app-identity.patch"
native_plist="$repo/runtime-patches/native_app_info.plist.in"
icon_source="$repo/runtime-patches/native_icon.swift"
wine_sha512=a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b
native_patch_sha256=f860b3a32a34d807319827c8a03298b1c24a705fa7075dc3a32cca7567c2d0ab
native_plist_sha256=cbb1781ded5b61bf7d3c7a87315aa54bce151879a073132216d043a9124eb7d5
icon_source_sha256=ee6957ce414da832719b31ec7eb66378477094af9004fa2a2f3bfc8f82fa03d0

verify() {
  algorithm=$1 expected=$2 input=$3
  actual=$(/usr/bin/shasum -a "$algorithm" "$input" | /usr/bin/awk '{print $1}')
  if [ "$actual" != "$expected" ]; then
    printf 'Unexpected SHA-%s for reviewed input %s\n' "$algorithm" "$input" >&2
    exit 65
  fi
}
for input in "$archive" "$steam_patch" "$dock_patch" "$native_patch" "$native_plist" "$icon_source" \
  "$seed/build/Makefile" "$seed/build/loader/main.o" "$seed/stage/caskrun-source/SOURCES.txt"; do
  [ -f "$input" ] || { printf 'Missing independent rebuild input: %s\n' "$input" >&2; exit 66; }
done
verify 512 "$wine_sha512" "$archive"
verify 256 408dade743a358886e76b6bb6e592ed6cb8fa67738c1b6d4b061d817c2cbe90f "$steam_patch"
verify 256 33811970ce63401baabaffd491ce0ac79c69ce740f0ad37687355041f2bd1844 "$dock_patch"
verify 256 "$native_patch_sha256" "$native_patch"
verify 256 "$native_plist_sha256" "$native_plist"
verify 256 "$icon_source_sha256" "$icon_source"
verify 256 408dade743a358886e76b6bb6e592ed6cb8fa67738c1b6d4b061d817c2cbe90f \
  "$seed/stage/caskrun-source/patches/wine-11.0-steam-cef-commandline.patch"
verify 256 33811970ce63401baabaffd491ce0ac79c69ce740f0ad37687355041f2bd1844 \
  "$seed/stage/caskrun-source/patches/wine-11.0-steam-dock.patch"

if [ "$mode" = check ]; then
  scratch=$(/usr/bin/mktemp -d "$repo/.build/.wine-native-patch-check.XXXXXX")
  trap 'rm -rf "$scratch"' EXIT HUP INT TERM
  /usr/bin/tar -xJf "$archive" -C "$scratch"
  cd "$scratch/wine-11.0"
  /usr/bin/patch --batch -p1 -F 0 < "$steam_patch"
  /usr/bin/patch --batch -p1 -F 0 < "$dock_patch"
  /usr/bin/patch --batch --dry-run -p1 -F 0 < "$native_patch"
  /usr/bin/sed 's/@PACKAGE_VERSION@/11.0/g' "$native_plist" > "$scratch/native.plist"
  /usr/bin/plutil -lint "$scratch/native.plist"
  printf 'Exact upstream archive, unchanged Steam patches, native identity patch and plist verified.\n'
  exit 0
fi
if [ -e "$output" ] || [ -L "$output" ]; then
  printf 'Isolated native app Wine build already exists; refusing to replace it.\n' >&2
  exit 73
fi
if [ "$(/usr/bin/uname -m)" != arm64 ] || [ "$(/usr/bin/arch -x86_64 /usr/bin/uname -m)" != x86_64 ]; then
  printf 'Apple Silicon and Rosetta are required.\n' >&2; exit 65
fi
/bin/cp -cRp "$seed" "$output"
# Generated make rules contain absolute source/stage locations. Rebase only
# the cloned configuration and preserve timestamps so unrelated objects stay
# reusable. Every changed Wine source file is rebuilt below.
python3 - "$seed" "$output" <<'PY'
from pathlib import Path
import os,sys
old,new=map(Path,sys.argv[1:])
for name in ['Makefile','config.status','config.log','include/config.h']:
 p=new/'build'/name
 if not p.is_file(): continue
 before=p.stat()
 data=p.read_bytes().replace(os.fsencode(old),os.fsencode(new))
 p.write_bytes(data)
 os.utime(p,ns=(before.st_atime_ns,before.st_mtime_ns))
PY
source="$output/src/wine-11.0"
build="$output/build"
stage="$output/stage"
cd "$source"
/usr/bin/patch --batch -p1 -F 0 < "$native_patch" > "$output/native-patch.log" 2>&1
cd "$build"
/usr/bin/make -j8 dlls/ntdll/ntdll.so dlls/winemac.drv/winemac.so loader/wine \
  > "$output/native-make.log" 2>&1
/usr/bin/make install > "$output/native-install.log" 2>&1
/usr/bin/sed 's/@PACKAGE_VERSION@/11.0/g' "$native_plist" > loader/native_app_info.plist
/usr/bin/plutil -lint loader/native_app_info.plist
/usr/bin/clang -target x86_64-apple-macos13 -o loader/WindowsApp loader/main.o \
  -Wl,-segalign,0x1000,-pagezero_size,0x1000,-sectcreate,__TEXT,__info_plist,loader/native_app_info.plist \
  -Wl,-no_pie,-image_base,0x200000000,-no_huge,-no_fixup_chains,-segalign,0x1000,-segaddr,WINE_RESERVE,0x1000,-segaddr,WINE_TOP_DOWN,0x7ff000000000 \
  > "$output/native-loader-link.log" 2>&1
/usr/bin/codesign --force --sign - loader/WindowsApp
/usr/bin/codesign --verify --strict loader/WindowsApp
/bin/cp -p loader/WindowsApp "$stage/lib/wine/x86_64-unix/WindowsApp"
# Retain the original Steam alias and behavior. Its NTDLL/Cocoa dependencies
# come from this rebuilt stage; CEF and process-role patches are unchanged.
/usr/bin/codesign --verify --strict "$stage/lib/wine/x86_64-unix/Steam"
source_meta="$stage/caskrun-source"
/bin/cp "$native_patch" "$native_plist" "$source_meta/patches/"
/bin/cp "$icon_source" "$source_meta/patches/"
cat >> "$source_meta/SOURCES.txt" <<MANIFEST

Original native app identity patch: patches/wine-11.0-native-app-identity.patch
Native app patch SHA-256: $native_patch_sha256
Native app loader plist: patches/native_app_info.plist.in
Native app plist SHA-256: $native_plist_sha256
Original native icon renderer: patches/native_icon.swift
Native icon renderer source SHA-256: $icon_source_sha256
Per-EXE app identities use only bounded metadata and artwork from the user's Windows executable.
Native loader alias: lib/wine/x86_64-unix/WindowsApp, ad-hoc signed.
Built in a separate APFS clone of the reviewed source build; original stage untouched.
MANIFEST
printf 'Isolated native identity Wine stage: %s\n' "$stage"
