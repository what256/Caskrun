#!/bin/sh
# Build the x86_64 crypto libraries needed by Wine's bcrypt and Schannel.
# Development experiment only; outputs stay under ignored .build/.
set -eu

if [ "$#" -ne 0 ]; then
  printf 'Usage: %s\n' "$0" >&2
  exit 64
fi

repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
probe=$repo/.build/tls-probe
sources=$probe/sources
build=$probe/build
prefix=$probe/prefix
logs=$probe/logs

if [ "$(uname -m)" != arm64 ] || [ "$(/usr/bin/arch -x86_64 /usr/bin/uname -m)" != x86_64 ]; then
  printf 'This development build needs Apple Silicon with Rosetta.\n' >&2
  exit 65
fi
if [ -e "$prefix/lib/libgnutls.30.dylib" ]; then
  printf 'GnuTLS is already built; refusing to replace it: %s\n' "$prefix" >&2
  exit 73
fi
mkdir -p "$sources" "$build" "$prefix" "$logs"

download_sha256() {
  url=$1
  expected=$2
  output=$3
  if [ ! -f "$output" ]; then
    partial=$output.part.$$
    if ! /usr/bin/curl -q --fail --location --retry 3 \
      --connect-timeout 20 --max-time 300 --max-filesize 104857600 \
      --proto '=https' --proto-redir '=https' --tlsv1.2 \
      --output "$partial" "$url"; then
      rm -f "$partial"
      exit 1
    fi
    mv "$partial" "$output"
  fi
  actual=$(/usr/bin/shasum -a 256 "$output" | /usr/bin/awk '{ print $1 }')
  if [ "$actual" != "$expected" ]; then
    printf 'SHA-256 mismatch for %s: expected %s, got %s\n' "$output" "$expected" "$actual" >&2
    exit 65
  fi
}

download_sha256 'https://ftp.gnu.org/gnu/gmp/gmp-6.3.0.tar.xz' \
  a3c2b80201b89e68616f4ad30bc66aee4927c3ce50e33929ca819d5c43538898 \
  "$sources/gmp-6.3.0.tar.xz"
download_sha256 'https://ftp.gnu.org/gnu/nettle/nettle-3.10.2.tar.gz' \
  fe9ff51cb1f2abb5e65a6b8c10a92da0ab5ab6eaf26e7fc2b675c45f1fb519b5 \
  "$sources/nettle-3.10.2.tar.gz"
download_sha256 'https://www.gnupg.org/ftp/gcrypt/gnutls/v3.8/gnutls-3.8.13.tar.xz' \
  ffed8ec1bf09c2426d4f14aae377de4753b53e537d685e604e99a8b16ca9c97e \
  "$sources/gnutls-3.8.13.tar.xz"

if [ ! -d "$build/gmp-6.3.0" ]; then /usr/bin/tar -xJf "$sources/gmp-6.3.0.tar.xz" -C "$build"; fi
if [ ! -d "$build/nettle-3.10.2" ]; then /usr/bin/tar -xzf "$sources/nettle-3.10.2.tar.gz" -C "$build"; fi
if [ ! -d "$build/gnutls-3.8.13" ]; then /usr/bin/tar -xJf "$sources/gnutls-3.8.13.tar.xz" -C "$build"; fi

# Portable C avoids x86_64 assembly relocations rejected by this macOS linker.
mkdir -p "$build/gmp-c-obj"
cd "$build/gmp-c-obj"
env CC='clang -arch x86_64' CFLAGS=-O2 \
  /usr/bin/arch -x86_64 /bin/sh "$build/gmp-6.3.0/configure" \
    --prefix="$prefix" --disable-assembly --disable-shared --enable-static --disable-cxx \
    > "$logs/gmp-c-configure.log" 2>&1
make -j8 > "$logs/gmp-c-build.log" 2>&1
make check > "$logs/gmp-c-check.log" 2>&1
make install > "$logs/gmp-c-install.log" 2>&1

mkdir -p "$build/nettle-obj"
cd "$build/nettle-obj"
env -u PKG_CONFIG_PATH PKG_CONFIG_LIBDIR="$prefix/lib/pkgconfig" \
  CC='clang -arch x86_64' CFLAGS=-O2 \
  CPPFLAGS="-I$prefix/include" LDFLAGS="-L$prefix/lib" \
  /usr/bin/arch -x86_64 /bin/sh "$build/nettle-3.10.2/configure" \
    --prefix="$prefix" --disable-shared --enable-static --disable-assembler --disable-fat \
    > "$logs/nettle-configure.log" 2>&1
make -j8 > "$logs/nettle-build.log" 2>&1
make check > "$logs/nettle-check.log" 2>&1
make install > "$logs/nettle-install.log" 2>&1

mkdir -p "$build/gnutls-obj"
cd "$build/gnutls-obj"
env -u PKG_CONFIG_PATH PKG_CONFIG_LIBDIR="$prefix/lib/pkgconfig" \
  CC='clang -arch x86_64' CFLAGS=-O2 \
  CPPFLAGS="-I$prefix/include" LDFLAGS="-L$prefix/lib" \
  /usr/bin/arch -x86_64 /bin/sh "$build/gnutls-3.8.13/configure" \
    --prefix="$prefix" --enable-shared --disable-static --disable-doc \
    --disable-tools --disable-tests --disable-nls --disable-libdane \
    --with-included-libtasn1 --with-included-unistring \
    --without-p11-kit --without-zlib \
    > "$logs/gnutls-configure.log" 2>&1
make -j8 > "$logs/gnutls-build.log" 2>&1
make install > "$logs/gnutls-install.log" 2>&1

if ! /usr/bin/file "$prefix/lib/libgnutls.30.dylib" | /usr/bin/grep -q x86_64; then
  printf 'GnuTLS output is not x86_64.\n' >&2
  exit 65
fi
if ! /usr/bin/nm -gU "$prefix/lib/libgnutls.30.dylib" | /usr/bin/grep -q _gnutls_pubkey_verify_hash2; then
  printf 'GnuTLS signature-verification symbol is missing.\n' >&2
  exit 65
fi
printf 'Project-local x86_64 GnuTLS prefix: %s\n' "$prefix"
