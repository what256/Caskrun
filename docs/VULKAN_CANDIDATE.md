# Isolated Vulkan runtime candidate

_23 September 2026. This is a local development candidate, not the runtime in the Caskrun app or development DMG._

The candidate is `.build/vulkan-candidate-runtime/`. It contains a complete independent Wine 11.0 build with the existing original Caskrun Steam patches, GnuTLS, x64 Unix Vulkan modules, x64 and x86 Windows Vulkan DLLs, a pinned Khronos x86_64 Vulkan Loader, and the official universal MoltenVK 1.4.2 dylib and unchanged portability ICD JSON. It uses Wine's existing graphics default. No Direct3D renderer or UI graphics choice was enabled. The current `.build/steam-wine-runtime-dev/`, `.build/Caskrun.app`, `dist/`, and the user's Steam bottle were not changed.

## Reproduction and provenance

The tracked commands build fresh Wine source and assemble a package without overwriting the active runtime:

```sh
./scripts/build-wine-vulkan-candidate.sh
./scripts/package-wine-runtime-vulkan-candidate.sh
```

The scripts refuse preexisting output directories. Build inputs are the independently sourced local artifacts listed in [LOCAL_WINE_RUNTIME.md](LOCAL_WINE_RUNTIME.md) and [GRAPHICS_PLAN.md](GRAPHICS_PLAN.md). They verify Wine 11.0's SHA-512, both original Steam patch hashes, the Steam loader template hash, Vulkan Loader/Headers source commits, and the reviewed Loader binary hash. The package script also checks MoltenVK's official release tar, dylib, and ICD JSON hashes. Exact URLs, revisions, and hashes are in the candidate's `SOURCES.txt`; Wine's corresponding source archive and patches are under `source/`. The package includes upstream Wine/dependency notices, Vulkan Loader and Headers notices, MoltenVK's Apache-2.0 license, and the official MoltenVK release tar. This records the development inputs, not a finished public-distribution compliance review.

The local package's dispatcher computes paths from its own location, sets `DYLD_FALLBACK_LIBRARY_PATH` for packaged FreeType/Vulkan libraries, and points `VK_DRIVER_FILES` at the **unchanged** MoltenVK ICD JSON beside `libMoltenVK.dylib`. The JSON uses `"./libMoltenVK.dylib"`; package symlinks are relative. The wrapper changes only the launched Wine process environment. The compiled x86_64 loader requires macOS 14.0 and links only to Apple system libraries; MoltenVK's x86_64 slice has a macOS 12.0 minimum and links to Apple frameworks. Copied Unix Mach-O files and graphics libraries were ad-hoc signed and passed `codesign --verify --strict`; the packaging audit found no absolute symlink or direct external Mach-O dependency. These are local integrity checks, not notarization or a clean-machine Gatekeeper test.

## Disposable-bottle result

A clone of the package at `.build/vulkan-candidate-relocated/Runtime Candidate/` reported `wine-11.0` with caller `DYLD_*`, Vulkan ICD, and Wine path overrides removed. The ARM64 Caskrun core `probe-runtime` command recognized the wrapper and its sibling `wineboot`. A new bottle under a separate `.build/vulkan-candidate-tests/core-home/` initialized successfully (`initialized: true`); its `system.reg` existed. The initialization record bound it to runtime identity `sha256:3c274a109b91481f5412632c9c85af5eeb88ffb9ff9a0a3ce3f9f659ac0e1214`. Re-initializing that idle bottle through the original candidate path returned `initialized: false` with the **same** runtime identity, demonstrating byte-identical package relocation for this path.

Self-authored Windows probes loaded `vulkan-1.dll` dynamically. Both were built from `.build/vulkan-candidate-tests/vulkan_device_dynamic.c` against the pinned headers and run inside that fresh bottle without caller graphics/library overrides. Each returned exit code 0:

| Probe | Observed output |
| --- | --- |
| Windows x64 | `loader_api=1.4.0 result=0`; `create_instance=0`; `enumerate_devices=0 count=1`; `device_0=Apple M5 Pro api=1.1.357` |
| Windows x86 | The same four results, including one Apple M5 Pro device at Vulkan 1.1.357. |

The x64 probe was held open for a live `lsof` check. Its process had **this candidate's** `runtime/lib/wine/x86_64-windows/winevulkan.dll`, `runtime/lib/wine/x86_64-unix/winevulkan.so`, `lib/vulkan/libvulkan.1.4.350.dylib`, and `lib/vulkan/libMoltenVK.dylib` mapped. This verifies which graphics libraries the successful x64 call used, rather than inferring it from the exit code. The x86 probe's separate process returned the same device output; the x86 module paths were not captured by that `lsof` sample. Probe logs, source, binaries, library-map log, Wineboot log, final runtime record, and build/configure logs remain under ignored `.build/vulkan-candidate-tests/`. The disposable bottle, relocation clone, failed packaging copy, and 5 GB of generated Wine build intermediates were removed after recording the evidence. The tracked build script can recreate the build tree from its pinned inputs.

Wineboot still logged RPC, USB, and Bluetooth service errors; these did not prevent initialization or the bounded Vulkan probes. The Mac desktop was locked, so no Wine GUI side effect was visually assessed. No candidate Wine process remained after the probes. No Steam or game was launched with this candidate.

After deleting the generated Wine build tree, a **second fresh bottle** initialized from the final package (`initialized: true`) and both Windows probes again exited 0 with the same one-device result. Its runtime record retained the same `sha256:3c274a109b91481f5412632c9c85af5eeb88ffb9ff9a0a3ce3f9f659ac0e1214` identity. The second disposable bottle was removed; `after-cleanup-*` logs and record in `.build/vulkan-candidate-tests/` preserve that independence check.

## What this does not establish

Vulkan device enumeration does **not** establish a Direct3D frame, Steam image quality, a working DXVK path, game installation, game play, or a suitable default renderer. The earlier [graphics probe](GRAPHICS_PLAN.md) found that DXVK 3.1.1 could not create a Vulkan instance with the MoltenVK portability driver, and MoltenVK's reported Vulkan 1.1 device and extension set fell short of DXVK 3.x requirements. Keep the current WineD3D default and leave renderer selection unavailable until a separately licensed translator draws and presents a verified Direct3D frame and a real game passes install/play/relaunch testing. Existing bottles are bound to their current runtime identity; this candidate is not a drop-in update for them. A public package would also need dependency-source and notice review, Developer ID signing/notarization, and clean-machine tests.
