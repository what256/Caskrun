#!/bin/sh
# Rebuild upstream Wine with project-local x86_64 GnuTLS support.
# Development experiment only; outputs stay under ignored .build/.
set -eu

if [ "$#" -ne 0 ]; then
  printf 'Usage: %s\n' "$0" >&2
  exit 64
fi

repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
tls="$repo/.build/tls-probe/prefix"
freetype="$repo/.build/wine-build-probe/deps-x86_64"
wine_src="$repo/.build/wine-build-probe/src/wine-11.0"
wine_build="$repo/.build/tls-wine-build"
wine_stage="$repo/.build/tls-wine-stage"
wine_archive="$repo/.build/wine-build-probe/wine-11.0.tar.xz"

if [ -e "$wine_build/Makefile" ] || [ -e "$wine_stage/bin/wine" ]; then
  printf 'Existing TLS Wine build or stage; refusing to replace it.\n' >&2
  exit 73
fi
for input in "$tls/lib/pkgconfig/gnutls.pc" "$tls/lib/libgnutls.30.dylib" \
  "$freetype/lib/pkgconfig/freetype2.pc" "$wine_src/configure" "$wine_archive"; do
  if [ ! -f "$input" ]; then
    printf 'Missing required input: %s\n' "$input" >&2
    exit 66
  fi
done
expected=a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b
actual=$(/usr/bin/shasum -a 512 "$wine_archive" | /usr/bin/awk '{ print $1 }')
if [ "$actual" != "$expected" ]; then
  printf 'Wine 11.0 source archive SHA-512 mismatch.\n' >&2
  exit 65
fi
/usr/bin/file "$tls/lib/libgnutls.30.dylib" | /usr/bin/grep -q x86_64
mkdir -p "$wine_build" "$wine_stage"
cd "$wine_build"
env -u PKG_CONFIG_PATH \
  PKG_CONFIG_LIBDIR="$tls/lib/pkgconfig:$freetype/lib/pkgconfig" \
  CC='/usr/bin/clang -target x86_64-apple-macos13' \
  CXX='/usr/bin/clang++ -target x86_64-apple-macos13' \
  BISON="$repo/.build/wine-build-probe/toolchain/bin/bison" \
  /usr/bin/arch -x86_64 /bin/sh "$wine_src/configure" \
    --build=x86_64-apple-darwin \
    --enable-archs=i386,x86_64 \
    --disable-tests \
    --without-x \
    --without-gstreamer \
    --with-gnutls \
    --prefix="$wine_stage" \
    > configure.log 2>&1

/usr/bin/grep -q '^#define SONAME_LIBGNUTLS' include/config.h
/usr/bin/grep -q '^#define HAVE_GNUTLS_CIPHER_INIT' include/config.h
make -j8 > make.log 2>&1
make install > make-install.log 2>&1

/usr/bin/file "$wine_stage/bin/wine" "$wine_stage/lib/wine/x86_64-unix/bcrypt.so"
printf 'Isolated TLS Wine stage: %s\n' "$wine_stage"
