#!/bin/sh
# Build a separate, diagnostic TLS-enabled runtime. Never replaces the stable package.
set -eu

if [ "$#" -ne 0 ]; then
  printf 'Usage: %s\n' "$0" >&2
  exit 64
fi

repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
case "${CASKRUN_TLS_VARIANT:-tls}" in
  tls)
    stage="$repo/.build/tls-wine-stage"
    wine_build="$repo/.build/tls-wine-build"
    default_package=tls-wine-runtime-dev
    ;;
  steam)
    stage="$repo/.build/wine-steam-11.0-cef-dock-dev/stage"
    wine_build="$repo/.build/wine-steam-11.0-cef-dock-dev/build"
    default_package=steam-wine-runtime-dev
    ;;
  *)
    printf 'Unknown TLS Wine variant.\n' >&2
    exit 64
    ;;
esac
tls="$repo/.build/tls-probe/prefix"
stable="$repo/.build/caskrun-wine-runtime-dev"
package_name=${CASKRUN_TLS_PACKAGE_NAME:-$default_package}
case "$package_name" in
  ''|.*|*/*) printf 'Invalid development package name: %s\n' "$package_name" >&2; exit 64 ;;
esac
target="$repo/.build/$package_name"
tmp="$target.tmp.$$"

if [ -e "$target" ] || [ -L "$target" ] || [ -e "$tmp" ]; then
  printf 'TLS package target already exists; refusing to replace it.\n' >&2
  exit 73
fi
for input in "$stage/bin/wine" "$stage/bin/wineboot" \
  "$tls/lib/libgnutls.30.dylib" "$stable/lib/libfreetype.6.dylib" \
  "$stable/runtime/share/wine/mono" "$stable/runtime/share/wine/gecko" \
  "$repo/scripts/wine-runtime-launch.sh" \
  "$wine_build/include/config.h" \
  "$repo/.build/tls-probe/sources/gmp-6.3.0.tar.xz" \
  "$repo/.build/tls-probe/sources/nettle-3.10.2.tar.gz" \
  "$repo/.build/tls-probe/sources/gnutls-3.8.13.tar.xz"; do
  if [ ! -e "$input" ]; then
    printf 'Missing package input: %s\n' "$input" >&2
    exit 66
  fi
done
if ! /usr/bin/grep -q '^#define HAVE_GNUTLS_CIPHER_INIT 1' \
  "$wine_build/include/config.h"; then
  printf 'TLS Wine build did not enable the GnuTLS bcrypt backend.\n' >&2
  exit 65
fi

find "$stage" -type l -print | while IFS= read -r link; do
  case "$(readlink "$link")" in
    /*) printf 'Absolute staged symlink: %s\n' "$link" >&2; exit 65 ;;
  esac
done

# Keep copied addon payloads tied to the Wine 11.0 source version.
verify_sha256() {
  expected=$1
  input=$2
  actual=$(/usr/bin/shasum -a 256 "$input" | /usr/bin/awk '{ print $1 }')
  if [ "$actual" != "$expected" ]; then
    printf 'SHA-256 mismatch for %s\n' "$input" >&2
    exit 65
  fi
}
verify_sha256 071f4b2887e1c97a11d791ff3d65be9429eed6dec4c2708888bfd546ba358e23 \
  "$stable/runtime/share/wine/mono/wine-mono-10.4.1-x86.msi"
verify_sha256 26cecc47706b091908f7f814bddb074c61beb8063318e9efc5a7f789857793d6 \
  "$stable/runtime/share/wine/gecko/wine-gecko-2.47.4-x86.msi"
verify_sha256 e590b7d988a32d6aa4cf1d8aa3aa3d33766fdd4cf4c89c2dcc2095ecb28d066f \
  "$stable/runtime/share/wine/gecko/wine-gecko-2.47.4-x86_64.msi"
verify_sha256 a3c2b80201b89e68616f4ad30bc66aee4927c3ce50e33929ca819d5c43538898 \
  "$repo/.build/tls-probe/sources/gmp-6.3.0.tar.xz"
verify_sha256 fe9ff51cb1f2abb5e65a6b8c10a92da0ab5ab6eaf26e7fc2b675c45f1fb519b5 \
  "$repo/.build/tls-probe/sources/nettle-3.10.2.tar.gz"
verify_sha256 ffed8ec1bf09c2426d4f14aae377de4753b53e537d685e604e99a8b16ca9c97e \
  "$repo/.build/tls-probe/sources/gnutls-3.8.13.tar.xz"

if [ "${CASKRUN_TLS_VARIANT:-tls}" = steam ]; then
  source_meta="$stage/caskrun-source"
  for input in "$stage/lib/wine/x86_64-unix/Steam" \
    "$source_meta/wine-11.0.tar.xz" \
    "$source_meta/patches/wine-11.0-steam-cef-commandline.patch" \
    "$source_meta/patches/wine-11.0-steam-dock.patch" \
    "$source_meta/patches/steam_info.plist.in" \
    "$source_meta/licenses/Wine-LICENSE.txt" \
    "$source_meta/licenses/Wine-LGPL-2.1.txt" \
    "$source_meta/SOURCES.txt"; do
    if [ ! -f "$input" ]; then
      printf 'Missing patched Wine source or alias: %s\n' "$input" >&2
      exit 66
    fi
  done
  wine_actual=$(/usr/bin/shasum -a 512 "$source_meta/wine-11.0.tar.xz" | /usr/bin/awk '{ print $1 }')
  if [ "$wine_actual" != a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b ]; then
    printf 'Patched Wine stage has an unexpected source archive.\n' >&2
    exit 65
  fi
  verify_sha256 408dade743a358886e76b6bb6e592ed6cb8fa67738c1b6d4b061d817c2cbe90f \
    "$source_meta/patches/wine-11.0-steam-cef-commandline.patch"
  verify_sha256 33811970ce63401baabaffd491ce0ac79c69ce740f0ad37687355041f2bd1844 \
    "$source_meta/patches/wine-11.0-steam-dock.patch"
  verify_sha256 b28004e117f9ee9d41716bb2d8b6fcda1d03d9f4a72f494706d02c248c17bb70 \
    "$source_meta/patches/steam_info.plist.in"
  /usr/bin/codesign --verify --strict "$stage/lib/wine/x86_64-unix/Steam"
fi

mkdir -p "$tmp/bin" "$tmp/lib" "$tmp/licenses"
trap 'if [ -d "$tmp" ]; then mv "$tmp" "$tmp.incomplete"; fi' EXIT HUP INT TERM
/bin/cp -cR "$stage" "$tmp/runtime"
if [ "${CASKRUN_TLS_VARIANT:-tls}" = steam ]; then
  /bin/mv "$tmp/runtime/caskrun-source" "$tmp/source"
fi
/bin/cp -cR "$stable/licenses/." "$tmp/licenses/"
/bin/cp "$stable/SOURCES.txt" "$tmp/SOURCES-base.txt"
mkdir -p "$tmp/runtime/share/wine"
/bin/cp -cR "$stable/runtime/share/wine/mono" "$tmp/runtime/share/wine/"
/bin/cp -cR "$stable/runtime/share/wine/gecko" "$tmp/runtime/share/wine/"
/bin/cp -c "$stable/lib/libfreetype.6.dylib" "$tmp/lib/"
/bin/cp -c "$tls/lib/libgnutls.30.dylib" "$tmp/lib/"
/usr/bin/install_name_tool -id '@rpath/libgnutls.30.dylib' "$tmp/lib/libgnutls.30.dylib"
/usr/bin/codesign --force --sign - "$tmp/lib/libgnutls.30.dylib"
/bin/cp "$repo/scripts/wine-runtime-launch.sh" "$tmp/bin/_dispatch"
chmod 755 "$tmp/bin/_dispatch"
ln -s _dispatch "$tmp/bin/wine"
ln -s _dispatch "$tmp/bin/wineboot"
ln -s _dispatch "$tmp/bin/wineserver"

# The build tree is local and the package is a development artifact, so ad-hoc
# sign its Unix Mach-O copies after staging. PE files are left untouched.
find "$tmp/runtime/bin" "$tmp/runtime/lib/wine/x86_64-unix" -type f -print | while IFS= read -r candidate; do
  description=$(/usr/bin/file -b "$candidate")
  case "$description" in
    *Mach-O*)
      case "$description" in
        *x86_64*) ;;
        *) printf 'Unexpected Mach-O architecture: %s\n' "$candidate" >&2; exit 65 ;;
      esac
      /usr/bin/codesign --force --sign - "$candidate"
      /usr/bin/codesign --verify --strict "$candidate"
      ;;
  esac
done
/usr/bin/codesign --verify --strict "$tmp/lib/libfreetype.6.dylib" "$tmp/lib/libgnutls.30.dylib"
if [ "${CASKRUN_TLS_VARIANT:-tls}" = steam ]; then
  /usr/bin/codesign --verify --strict "$tmp/runtime/lib/wine/x86_64-unix/Steam"
fi

# Wine dynamically opens GnuTLS and FreeType. Reject direct Mach-O dependencies
# that would escape the package or macOS system frameworks.
find "$tmp/runtime/bin" "$tmp/runtime/lib/wine/x86_64-unix" "$tmp/lib" -type f -print | while IFS= read -r candidate; do
  description=$(/usr/bin/file -b "$candidate")
  case "$description" in
    *Mach-O*)
      /usr/bin/otool -L "$candidate" | /usr/bin/awk 'NR > 1 { print $1 }' | while IFS= read -r dependency; do
        case "$dependency" in
          /usr/lib/*|/System/Library/*|@rpath/*|@loader_path/*|@executable_path/*) ;;
          *) printf 'External Mach-O dependency in %s: %s\n' "$candidate" "$dependency" >&2; exit 65 ;;
        esac
      done
      ;;
  esac
done

if [ "${CASKRUN_TLS_VARIANT:-tls}" = steam ]; then
  /bin/cp "$tmp/source/licenses/Wine-LICENSE.txt" "$tmp/licenses/Wine-LICENSE.txt"
  /bin/cp "$tmp/source/licenses/Wine-LGPL-2.1.txt" "$tmp/licenses/Wine-LGPL-2.1.txt"
else
  /bin/cp "$repo/.build/wine-build-probe/src/wine-11.0/LICENSE" "$tmp/licenses/Wine-LICENSE.txt"
  /bin/cp "$repo/.build/wine-build-probe/src/wine-11.0/COPYING.LIB" "$tmp/licenses/Wine-LGPL-2.1.txt"
fi
/bin/cp "$repo/.build/tls-probe/build/gnutls-3.8.13/COPYING" "$tmp/licenses/GnuTLS-COPYING.txt"
/bin/cp "$repo/.build/tls-probe/build/gnutls-3.8.13/COPYING.LESSERv2" "$tmp/licenses/GnuTLS-LGPL-2.1.txt"
/bin/cp "$repo/.build/tls-probe/build/gnutls-3.8.13/README.md" "$tmp/licenses/GnuTLS-README.md"
/bin/cp "$repo/.build/tls-probe/build/nettle-3.10.2/COPYING.LESSERv3" "$tmp/licenses/Nettle-LGPL-3.txt"
/bin/cp "$repo/.build/tls-probe/build/nettle-3.10.2/COPYINGv2" "$tmp/licenses/Nettle-GPL-2.txt"
/bin/cp "$repo/.build/tls-probe/build/nettle-3.10.2/COPYINGv3" "$tmp/licenses/Nettle-GPL-3.txt"
/bin/cp "$repo/.build/tls-probe/build/gmp-6.3.0/COPYING.LESSERv3" "$tmp/licenses/GMP-LGPL-3.txt"
/bin/cp "$repo/.build/tls-probe/build/gmp-6.3.0/COPYINGv2" "$tmp/licenses/GMP-GPL-2.txt"
/bin/cp "$repo/.build/tls-probe/build/gmp-6.3.0/COPYINGv3" "$tmp/licenses/GMP-GPL-3.txt"

cat > "$tmp/SOURCES.txt" <<'EOF'
Development-only TLS-enabled Wine runtime experiment. Not a signed/notarized release.
Wine 11.0: https://dl.winehq.org/wine/source/11.0/wine-11.0.tar.xz
SHA-512: a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b
GnuTLS 3.8.13: https://www.gnupg.org/ftp/gcrypt/gnutls/v3.8/gnutls-3.8.13.tar.xz
SHA-256: ffed8ec1bf09c2426d4f14aae377de4753b53e537d685e604e99a8b16ca9c97e
Nettle 3.10.2: https://ftp.gnu.org/gnu/nettle/nettle-3.10.2.tar.gz
SHA-256: fe9ff51cb1f2abb5e65a6b8c10a92da0ab5ab6eaf26e7fc2b675c45f1fb519b5
GMP 6.3.0: https://ftp.gnu.org/gnu/gmp/gmp-6.3.0.tar.xz
SHA-256: a3c2b80201b89e68616f4ad30bc66aee4927c3ce50e33929ca819d5c43538898
FreeType, Wine Mono, and Wine Gecko inputs copied from the existing independent
development package; see docs/LOCAL_WINE_RUNTIME.md and its SOURCES.txt.
All dependencies were built or downloaded independently of CrossOver.
EOF
if [ "${CASKRUN_TLS_VARIANT:-tls}" = steam ]; then
  cat >> "$tmp/SOURCES.txt" <<'EOF'

Caskrun's reviewed Wine 11.0 Steam patches, signed native Steam loader alias,
exact corresponding Wine source archive, patch files, plist template, and Wine
license texts are in source/; see source/SOURCES.txt. The original Steam icon
is extracted from the user's own Steam.exe at launch, not bundled here.
EOF
  # The wrapper must run from its new package path with no caller-provided
  # Wine or dynamic-library search paths. This is a narrow relocation check;
  # disposable-bottle and Steam GUI tests remain separate.
  version=$(/usr/bin/env \
    -u DYLD_LIBRARY_PATH -u DYLD_FALLBACK_LIBRARY_PATH \
    -u WINEDLLPATH -u WINEDATADIR -u WINELOADER -u WINESERVER \
    -u CASKRUN_STEAM_MODE -u CASKRUN_STEAM_ICON \
    "$tmp/bin/wine" --version)
  if [ "$version" != wine-11.0 ]; then
    printf 'Relocated Steam Wine package did not report wine-11.0: %s\n' "$version" >&2
    exit 65
  fi
fi

mv "$tmp" "$target"
trap - EXIT HUP INT TERM
printf 'TLS diagnostic runtime: %s\n' "$target"
