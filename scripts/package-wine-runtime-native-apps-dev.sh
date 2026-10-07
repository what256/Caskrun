#!/bin/sh
# Package the isolated per-EXE native identity build without replacing Steam's package.
set -eu
[ "$#" -eq 0 ] || { printf 'Usage: %s\n' "$0" >&2; exit 64; }
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
exec env CASKRUN_TLS_VARIANT=native-apps \
  CASKRUN_TLS_PACKAGE_NAME=native-apps-wine-runtime-dev \
  "$repo/scripts/package-wine-runtime-tls-dev.sh"
