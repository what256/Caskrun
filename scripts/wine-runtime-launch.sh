#!/bin/sh
# Development Wine entrypoint. The packager installs this at bin/_dispatch and
# links bin/wine, bin/wineboot, and bin/wineserver to it.
set -eu

case "${0##*/}" in
  wine|wineboot|wineserver) command_name=${0##*/} ;;
  *) printf 'Unsupported Wine runtime command: %s\n' "${0##*/}" >&2; exit 64 ;;
esac

runtime_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
wine_root=$runtime_root/runtime
freetype_dir=$runtime_root/lib

if [ ! -x "$wine_root/bin/$command_name" ] || [ ! -f "$freetype_dir/libfreetype.6.dylib" ]; then
  printf 'Incomplete Caskrun development Wine runtime at %s\n' "$runtime_root" >&2
  exit 66
fi

# Wine loads FreeType with dlopen("libfreetype.6.dylib"), so its Mach-O
# install name alone cannot direct this lookup. Keep the search path inside
# this entrypoint; callers do not need to configure dyld themselves.
DYLD_FALLBACK_LIBRARY_PATH=$freetype_dir${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}
WINEDLLPATH=$wine_root/lib/wine${WINEDLLPATH:+:$WINEDLLPATH}
WINEDATADIR=$wine_root/share/wine
WINELOADER=$wine_root/bin/wine
WINESERVER=$wine_root/bin/wineserver
export DYLD_FALLBACK_LIBRARY_PATH WINEDLLPATH WINEDATADIR WINELOADER WINESERVER

exec "$wine_root/bin/$command_name" "$@"
