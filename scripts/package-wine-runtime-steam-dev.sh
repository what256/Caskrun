#!/bin/sh
# Package the isolated, reviewed Steam Wine build. Never touches the stable app.
set -eu

if [ "$#" -ne 0 ]; then
  printf 'Usage: %s\n' "$0" >&2
  exit 64
fi

repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
exec env CASKRUN_TLS_VARIANT=steam \
  CASKRUN_TLS_PACKAGE_NAME=steam-wine-runtime-dev \
  "$repo/scripts/package-wine-runtime-tls-dev.sh"
