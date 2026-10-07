#!/bin/sh
# Candidate-only Wine entrypoint: resolve graphics dependencies from this tree.
set -eu

case "${0##*/}" in
  wine|wineboot|wineserver) command_name=${0##*/} ;;
  *) printf 'Unsupported Wine runtime command: %s\n' "${0##*/}" >&2; exit 64 ;;
esac

runtime_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
wine_root=$runtime_root/runtime
native_lib=$runtime_root/lib
vulkan_dir=$native_lib/vulkan

for input in "$wine_root/bin/$command_name" \
  "$native_lib/libfreetype.6.dylib" "$native_lib/libgnutls.30.dylib" \
  "$vulkan_dir/libvulkan.1.dylib" "$vulkan_dir/libMoltenVK.dylib" \
  "$vulkan_dir/MoltenVK_icd.json"; do
  if [ ! -e "$input" ]; then
    printf 'Incomplete Caskrun Vulkan candidate at %s\n' "$runtime_root" >&2
    exit 66
  fi
done

# Wine dlopens the Vulkan loader and FreeType by soname. The Vulkan loader
# resolves the official relative MoltenVK ICD path beside its JSON manifest.
# These paths exist only in this child process and follow package relocation.
DYLD_FALLBACK_LIBRARY_PATH=$vulkan_dir:$native_lib${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}
VK_DRIVER_FILES=$vulkan_dir/MoltenVK_icd.json
WINEDLLPATH=$wine_root/lib/wine${WINEDLLPATH:+:$WINEDLLPATH}
WINEDATADIR=$wine_root/share/wine
WINELOADER=$wine_root/bin/wine
WINESERVER=$wine_root/bin/wineserver
export DYLD_FALLBACK_LIBRARY_PATH VK_DRIVER_FILES WINEDLLPATH WINEDATADIR WINELOADER WINESERVER

exec "$wine_root/bin/$command_name" "$@"
