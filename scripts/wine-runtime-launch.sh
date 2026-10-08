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

# Wine searches the canonical ntdll library directory before WINEDLLPATH.
# The switchable package therefore contains two real runtime views. Merely
# prepending translator DLLs to WINEDLLPATH cannot override builtins reliably.
# The server's bytes are common to both views and its control path stays stable.
if [ "$command_name" != wineserver ]; then
  case "${CASKRUN_GRAPHICS_BACKEND:-auto}" in
    auto|wined3d) selected_backend=wined3d ;;
    dxmt)
      if [ ! -f "$runtime_root/graphics-layout.txt" ] ||
         [ "$(cat "$runtime_root/graphics-layout.txt")" != caskrun-switchable-graphics-v1 ]; then
        printf 'This Windows engine does not include switchable DXMT support.\n' >&2
        exit 65
      fi
      selected_backend=dxmt
      wine_root=$runtime_root/components/dxmt/runtime
      ;;
    *) printf 'Unsupported Caskrun graphics backend.\n' >&2; exit 64 ;;
  esac
  # Native per-app bundles copy Wine's loader out of the runtime. Keep the
  # library it loads in the selected view; accept only our two exact paths.
  case "${CASKRUN_NATIVE_NTDLL:-}" in
    '') ;;
    "$runtime_root/runtime/lib/wine/x86_64-unix/ntdll.so"|\
    "$runtime_root/components/dxmt/runtime/lib/wine/x86_64-unix/ntdll.so")
      CASKRUN_NATIVE_NTDLL=$wine_root/lib/wine/x86_64-unix/ntdll.so
      export CASKRUN_NATIVE_NTDLL
      ;;
    *) printf 'Native app library does not match this Windows engine.\n' >&2; exit 65 ;;
  esac
  CASKRUN_GRAPHICS_BACKEND=$selected_backend
  export CASKRUN_GRAPHICS_BACKEND
fi

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
WINESERVER=$runtime_root/runtime/bin/wineserver
export DYLD_FALLBACK_LIBRARY_PATH WINEDLLPATH WINEDATADIR WINELOADER WINESERVER

exec "$wine_root/bin/$command_name" "$@"
