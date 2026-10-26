# Vulkan and hybrid DXVK evidence

_26 October 2026. These are isolated candidate API results. Final application
packaging, storefronts, games and performance are separate acceptance gates._

## Current successful route

The coordinated Wine 11 build supplies both Windows Vulkan architectures,
matching Unix modules, the pinned official Vulkan loader, and MoltenVK 1.4.2.
The successful hybrid uses
[Metalsharp DXVK-MacOS 3.1-macos1.0](https://github.com/metalsharp/DXVK-MacOS/tree/bd8921a3db6c4928f5b34672791121f7c14e38c2)
for D3D10/11 and source-built
[tairasu D9VK 1.10.3-20230507-async-macos](https://github.com/tairasu/d9vk/tree/7437b616f05bc678d2ff4b4b75832f9cbddeed75)
for D3D9. Both translator sources use the Zlib license. Official
[MoltenVK 1.4.2](https://github.com/KhronosGroup/MoltenVK/tree/db66022459ffb663aa2b50f6b018bc2e124f5edf)
uses Apache-2.0 and came from the pinned official Vulkan SDK 1.4.363.0.

The original probe matrix passed in **one coherent hybrid runtime view**:

| API | Windows callers | Each architecture's result |
| --- | --- | --- |
| D3D11 | x64 and x86 WoW64 | 242 checks, 56 readback pixels, 14 Present calls; feature levels 10/11, shaders, readback, multiple windows, resize, destroyed-window handling and recreation. |
| D3D9 | x64 and x86 WoW64 | 154 checks, 24 exact pixels, 6 Present calls; HAL fixed-function triangle, readback and Reset/resize. |

The same owned processes mapped their exact selected translator images, the
reviewed MoltenVK and loader. No WineD3D or WineMetal was present at the
pre-compiler renderer identity gate. All ten prefix graphics DLLs and all eight
hybrid view PEs remained unchanged. The four raw result streams and typed
summary are bound to the component receipt; shareable metadata records hashes
and fixed result fields, not private mapped paths.

The eight managed DLLs are D3D9, D3D10Core, D3D11 and DXGI in two Windows
architectures. D3D8 remains the original Wine implementation and is not forced
through this hybrid. The selected native PE copies receive only Wine's builtin
DOS-stub marker, with original/runtime hashes and exact changed-byte checks.
The app/core fixes the package-relative ICD, and secondary view server entries
point to the same common protocol-932 server.

## Failed paths and limits

The modern fork's D3D9 device/clear path worked but its fixed-function triangle
failed. The legacy source-built D3D9 module passed that exact test and the full
lifecycle gates, which motivates the per-API combination rather than claiming
the modern module works. Older stock DXVK/MoltenVK failures remain historical
evidence below. KosmicKrisp is a separate research route and is not the driver
used by this successful matrix.

The measured MoltenVK route lacks geometry shader, transform-feedback,
nullDescriptor and robustBufferAccess2 capabilities required by some titles.
A successful API probe is not a guarantee for every Direct3D 9/10/11 program.
There is no D3D8 or D3D12 acceptance, title compatibility, FPS comparison,
visible game-pixel check or store sign-in result in this pass. Keep existing
working manual settings until testing a separate bottle; see
[the manual checklist](NEXT_MANUAL_TESTS.md) and
[backend status](GRAPHICS_BACKEND_STATUS.md).

## Historical candidate — 23 September 2026

_23 September 2026. This was an isolated development candidate, not the runtime then included in the Caskrun app or development DMG._

That historical candidate was built at `.build/vulkan-candidate-runtime/`. It contains a complete independent Wine 11.0 build with the existing original Caskrun Steam patches, GnuTLS, x64 Unix Vulkan modules, x64 and x86 Windows Vulkan DLLs, a pinned Khronos x86_64 Vulkan Loader, and the official universal MoltenVK 1.4.2 dylib and unchanged portability ICD JSON. It uses Wine's existing graphics default. No Direct3D renderer or UI graphics choice was enabled. The then-active `.build/steam-wine-runtime-dev/`, `.build/Caskrun.app`, `dist/`, and the user's Steam bottle were not changed.

### Historical reproduction and provenance

The tracked commands build fresh Wine source and assemble a package without overwriting the active runtime:

```sh
./scripts/build-wine-vulkan-candidate.sh
./scripts/package-wine-runtime-vulkan-candidate.sh
```

The scripts refuse preexisting output directories. Build inputs are the independently sourced local artifacts listed in [LOCAL_WINE_RUNTIME.md](LOCAL_WINE_RUNTIME.md) and [GRAPHICS_PLAN.md](GRAPHICS_PLAN.md). They verify Wine 11.0's SHA-512, both original Steam patch hashes, the Steam loader template hash, Vulkan Loader/Headers source commits, and the reviewed Loader binary hash. The package script also checks MoltenVK's official release tar, dylib, and ICD JSON hashes. Exact URLs, revisions, and hashes are in the candidate's `SOURCES.txt`; Wine's corresponding source archive and patches are under `source/`. The package includes upstream Wine/dependency notices, Vulkan Loader and Headers notices, MoltenVK's Apache-2.0 license, and the official MoltenVK release tar. This records the development inputs, not a finished public-distribution compliance review.

The local package's dispatcher computes paths from its own location, sets `DYLD_FALLBACK_LIBRARY_PATH` for packaged FreeType/Vulkan libraries, and points `VK_DRIVER_FILES` at the **unchanged** MoltenVK ICD JSON beside `libMoltenVK.dylib`. The JSON uses `"./libMoltenVK.dylib"`; package symlinks are relative. The wrapper changes only the launched Wine process environment. The compiled x86_64 loader requires macOS 14.0 and links only to Apple system libraries; MoltenVK's x86_64 slice has a macOS 12.0 minimum and links to Apple frameworks. Copied Unix Mach-O files and graphics libraries were ad-hoc signed and passed `codesign --verify --strict`; the packaging audit found no absolute symlink or direct external Mach-O dependency. These are local integrity checks, not notarization or a clean-machine Gatekeeper test.

### Historical disposable-bottle result

A clone of the package at `.build/vulkan-candidate-relocated/Runtime Candidate/` reported `wine-11.0` with caller `DYLD_*`, Vulkan ICD, and Wine path overrides removed. The ARM64 Caskrun core `probe-runtime` command recognized the wrapper and its sibling `wineboot`. A new bottle under a separate `.build/vulkan-candidate-tests/core-home/` initialized successfully (`initialized: true`); its `system.reg` existed. The initialization record bound it to runtime identity `sha256:3c274a109b91481f5412632c9c85af5eeb88ffb9ff9a0a3ce3f9f659ac0e1214`. Re-initializing that idle bottle through the original candidate path returned `initialized: false` with the **same** runtime identity, demonstrating byte-identical package relocation for this path.

Self-authored Windows probes loaded `vulkan-1.dll` dynamically. Both were built from `.build/vulkan-candidate-tests/vulkan_device_dynamic.c` against the pinned headers and run inside that fresh bottle without caller graphics/library overrides. Each returned exit code 0:

| Probe | Observed output |
| --- | --- |
| Windows x64 | `loader_api=1.4.0 result=0`; `create_instance=0`; `enumerate_devices=0 count=1`; `device_0=(host GPU name omitted) api=1.1.357` |
| Windows x86 | The same four results, including one (host GPU name omitted) device at Vulkan 1.1.357. |

The x64 probe was held open for a live `lsof` check. Its process had **this candidate's** `runtime/lib/wine/x86_64-windows/winevulkan.dll`, `runtime/lib/wine/x86_64-unix/winevulkan.so`, `lib/vulkan/libvulkan.1.4.350.dylib`, and `lib/vulkan/libMoltenVK.dylib` mapped. This verifies which graphics libraries the successful x64 call used, rather than inferring it from the exit code. The x86 probe's separate process returned the same device output; the x86 module paths were not captured by that `lsof` sample. Probe logs, source, binaries, library-map log, Wineboot log, final runtime record, and build/configure logs were recorded under ignored `.build/vulkan-candidate-tests/` at the time; generated evidence is disposable after owner checks. The disposable bottle, relocation clone, failed packaging copy, and 5 GB of generated Wine build intermediates were removed after recording the evidence. The tracked build script can recreate the build tree from its pinned inputs.

Wineboot still logged RPC, USB, and Bluetooth service errors; these did not prevent initialization or the bounded Vulkan probes. The Mac desktop was locked, so no Wine GUI side effect was visually assessed. No candidate Wine process remained after the probes. No Steam or game was launched with this candidate.

After deleting the generated Wine build tree, a **second fresh bottle** initialized from the final package (`initialized: true`) and both Windows probes again exited 0 with the same one-device result. Its runtime record retained the same `sha256:3c274a109b91481f5412632c9c85af5eeb88ffb9ff9a0a3ce3f9f659ac0e1214` identity. The second disposable bottle was removed; `after-cleanup-*` logs and record in `.build/vulkan-candidate-tests/` preserve that independence check.

### Limits of that historical candidate

Vulkan device enumeration does **not** establish a Direct3D frame, Steam image quality, a working DXVK path, game installation, game play, or a suitable default renderer. The earlier [graphics probe](GRAPHICS_PLAN.md) found that DXVK 3.1.1 could not create a Vulkan instance with the MoltenVK portability driver, and MoltenVK's reported Vulkan 1.1 device and extension set fell short of DXVK 3.x requirements. That candidate retained WineD3D and left renderer selection unavailable pending separate translator/device/frame validation and real-game testing. Existing bottles are bound to their current runtime identity; that candidate was not a drop-in update for them. A public package would also need dependency-source and notice review, Developer ID signing/notarization, and clean-machine tests.
