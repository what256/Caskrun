# Graphics compatibility path

_Research began 23 September 2026. Current status is updated 26 October; the
dated research sections below retain historical observations and plans._

Caskrun retains WineD3D as its conservative fallback and optional verified DXMT
with frame target/MetalFX spatial controls. The current coordinated candidate
also supplies a separately pinned hybrid DXVK view and an original MSync port.
All four D3D9/D3D11 x86/x64 GPU cells passed through the optimized Core, saved
DXVK selection and packaged dispatcher, with mapped-module identity, readback,
presentation, lifecycle, immutable prefix DLLs and refused busy edits. MSync's
off/on WineD3D/DXMT matrix passed 296 checks and remains off by default. Existing
manual choices and the Collection/Discover layout are preserved.

D3D11's per-architecture result is 242 checks, 56 readback pixels and 14 Present
calls; D3D9's is 154 checks, 24 readback pixels and 6 Present calls. D3D8 remains
Wine's implementation, and the hybrid establishes no D3D12 or universal game
support. D3DMetal's accepted official private input failed its factory gate and
is unavailable; host-ABI compatibility remains unresolved. No new installed
artifact or game result is implied. See [backend status](GRAPHICS_BACKEND_STATUS.md),
[Vulkan evidence](VULKAN_CANDIDATE.md), [app integration](DXMT_APP_INTEGRATION.md)
and [manual checks](NEXT_MANUAL_TESTS.md).

The historical sections below explain earlier failures and the path to DXMT.
Their uses of “current,” “next” or “unavailable” refer to their dated snapshots,
not the current candidate. Historical tables are preserved as evidence.

## Historical isolated Vulkan and Direct3D probe — 23 September 2026

An ignored, disposable prototype at `.build/graphics-probe/` shows that the missing Vulkan layer can be **built for the existing x86_64 Wine 11 runtime on Apple Silicon**, but does not show that Direct3D works. The packaged `.build/Caskrun.app`, installed applications, source runtime, and build scripts were not changed. The test used an APFS clone of the independent development Wine runtime with only locally built Wine Vulkan modules and upstream Vulkan components overlaid. The clone and test bottle were moved to macOS Trash after preserving evidence; this was not a release package.

| Check | Observed result |
| --- | --- |
| Khronos [Vulkan Loader](https://github.com/KhronosGroup/Vulkan-Loader) and [Headers](https://github.com/KhronosGroup/Vulkan-Headers), both pinned to `v1.4.350` | Built an x86_64 `libvulkan.1.dylib` with macOS 14.0 minimum and ad-hoc signed the disposable overlay copy used in the test. The retained build artifact is unsigned. The working build requires `CMAKE_APPLE_SILICON_PROCESSOR=x86_64` as well as `CMAKE_OSX_ARCHITECTURES=x86_64`; otherwise CMake selects the wrong assembler path on this arm64 host. |
| Official [MoltenVK 1.4.2 macOS release](https://github.com/KhronosGroup/MoltenVK/releases/tag/v1.4.2) | Its universal `libMoltenVK.dylib` has an x86_64 slice; the official ICD manifest marks it as a portability driver. The x86_64 native Vulkan probe enumerated one Apple M5 Pro physical device. |
| Independently sourced [Wine 11.0](https://gitlab.winehq.org/wine/wine/-/releases/wine-11.0) with `--with-vulkan` | A separate build found `libvulkan.1.dylib` and built `winevulkan`/`win32u` modules. In a fresh disposable bottle, a self-authored Windows x64 Vulkan executable returned exit 0: loader API 1.4.0, instance creation 0, one Apple M5 Pro device reporting Vulkan **1.1.357**. Wineboot also completed. |
| Built-in WineD3D D3D11 probe | `D3D11CreateDevice` returned `0x80004005`; Wine reported that none of the requested 10.0/11.0 feature levels was supported by its current shader backend. |
| Official [DXVK 3.1.1 release](https://github.com/doitsujin/dxvk/releases/tag/v3.1.1), app-local x64 `d3d11.dll` and `dxgi.dll` with native Wine overrides | Both DLLs loaded, then `D3D11CreateDevice` returned `0x80004005`. DXVK logged `DxvkInstance::createInstance: Failed to create Vulkan instance`. Wine's Vulkan trace shows that DXVK requested a Vulkan 1.3 instance without `VK_KHR_portability_enumeration` or its enumeration flag. The [Khronos loader documentation](https://github.com/KhronosGroup/Vulkan-Loader/blob/main/docs/LoaderDriverInterface.md#loader-implementation-of-vk_khr_portability_enumeration) says portability drivers are skipped without both, which explains this observed failure. The direct Vulkan probe did set both and succeeded. This is not a working DXVK path. |

Even after a correct portability-enumeration fix, the device's reported Vulkan 1.1.357 is below [DXVK 3.x's current Vulkan 1.4 driver requirement](https://github.com/doitsujin/dxvk/wiki/Driver-support). MoltenVK's observed extension list also lacks `VK_EXT_depth_clip_enable` and `VK_EXT_transform_feedback`, which the same DXVK guidance lists as required. Those are additional compatibility gaps; the current test stopped before DXVK device creation. An older DXVK release was not tested and must not be treated as a validated fallback.

The traces, executables, self-authored C probes, build logs, wrapper snapshot, and notices are under `.build/graphics-probe/evidence/`. Reusable build artifacts and upstream archives remain under the same ignored probe directory, now about 416 MB rather than 3.8 GB. Pinned official download SHA-256 values are `f95765a6229cb7b915990a2890ce12ebe36a730b021545d3d52ae69ce4c4024e` (MoltenVK tar) and `40565b4a724aadc4433fa4e010b4b23916d9b1f1baeee64e17186db94f54e608` (DXVK tar). Loader and Headers source commits are `a9e72c66d5cb79911eb9a9063bf4016dd0a3a123` and `8864cdc896bbc2a9b6eb36b3218fc9ef57908d77`. License copies are in `evidence/licenses/`: Wine is LGPL-2.1-or-later; Vulkan Loader mostly Apache-2.0 with permissive exceptions; Headers Apache-2.0/MIT; MoltenVK Apache-2.0; DXVK zlib. Distribution would require a full dependency and notice review.

### Historical integration contract for the next renderer experiment

1. Rebuild the **whole** independent Wine runtime against pinned x86_64 Vulkan Loader and MoltenVK development files. Stage matching x86_64 Unix modules and Windows PE DLLs, and preserve Wine's separate 32-bit PE path. Do not graft the prototype's two modules into a shipped Wine build without a full rebuild and smoke suite.
2. Put `libvulkan.1.dylib`, `libMoltenVK.dylib`, and its unchanged portability ICD JSON inside the runtime. Resolve their absolute paths from the runtime's own location when launching, then set `DYLD_FALLBACK_LIBRARY_PATH` and `VK_DRIVER_FILES` only inside the Wine entrypoint. The successful probe used no caller graphics environment overrides. Verify Mach-O architectures, deployment target, linkage, relocation, signatures, hashes, and license notices after packaging.
3. Choose a renderer only after a self-authored Direct3D test creates a device, draws a frame, and produces a visible result in a fresh bottle. Verify x64 and x86 separately; record which DLL actually loaded and its renderer log. Then test a real game through install, launch, play, exit, and relaunch before assigning a default for that title. Leave the ordinary Install → Play flow free of Wine/Vulkan choices.

At the initial Vulkan snapshot, initialized bottles were bound to the bundled runtime-tree digest and the only reviewed migration was an idle, verified Steam transition. The later [DXMT app integration](DXMT_APP_INTEGRATION.md) adds a reviewed exact native-runtime transition with idle checks and rollback. A different Vulkan candidate still needs fresh disposable bottles and its own approved source/destination identities before existing bottles can use it; replacing the DMG alone does not approve a migration.

The separate [DXMT upstream build guide](https://github.com/3Shain/dxmt/blob/main/docs/DEVELOPMENT.md) is a plausible next D3D10/11 experiment because it targets Metal directly and avoids this Vulkan device limit. Its documented Wine cross-build requires **Meson 1.3+**, **LLVM 15 exactly with x86_64 headers and static libraries**, Xcode 16+, Wine 8+ build files, and a Windows cross-compiler. At that initial review, the Meson/Ninja/LLVM toolchain and DXMT probes were not staged; the later gates below record their completed build and API tests. The separately published [DXMT license](https://github.com/3Shain/dxmt/blob/main/LICENSE) is LGPL-2.1-or-later and credits Feifan He **for CodeWeavers**. Review that upstream provenance against the project's no-CrossOver-code requirement before including it. If approved, pin a DXMT source revision and toolchain, build x64 PE DLLs plus its x64 Unix library in ignored `.build/`, and run the same device/frame/visual tests; 32-bit PE DLLs are a separate build and depend on the x64 Unix library.

### Historical DXMT gate 1: the engine hides what it must export — 8 October 2026

The compatibility plan recorded that DXMT needs exported Wine `winemac` APIs. That
is now measured against this project's own engine rather than taken from a
document, and the blocker is specific and fixable.

DXMT's unix layer resolves the driver two ways. It first looks for a
`macdrv_functions` structure, which is a CrossOver export and is absent from
upstream Wine. Failing that it falls back to `dlsym(RTLD_DEFAULT, ...)` for
individual names. Auditing every `dlsym` call at the pinned revision shows the
fallback resolves exactly **five** functions: `get_win_data`,
`release_win_data`, `macdrv_view_create_metal_view`,
`macdrv_view_get_metal_layer` and `macdrv_view_release_metal_view`. The other
names in that structure, including `macdrv_create_metal_device` and
`macdrv_get_cocoa_window`, appear only as structure members and are never looked
up on the upstream path.

In the packaged engine all five exist in `winemac.so` and all five are local
symbols: the library exports two symbols out of 1905 entries. An original patch
giving those five declarations default visibility raises the exported count to 7,
and a rebuilt `winemac.so` from a clone of this project's configured Wine 11.0
build tree confirms it, with the unused names left local.

A second defect sits on the DXMT side. It declares its own four-pointer copy of
`struct macdrv_win_data` and reads the client view from offset 24. Wine 11.0 has
no separate `cocoa_view`: the client view is `client_view` at offset 16, and
offset 24 begins a `struct window_rects`. Built unmodified against this driver,
DXMT would hand window coordinates to `macdrv_view_create_metal_view` as a view
pointer. A second patch declares only the fields that file reads, with Wine
11.0's names and order.

Both patches are recorded in [runtime-patches](../runtime-patches/README.md).
Changing the driver changes the runtime-tree digest existing bottles are bound
to, so the later build and rendering gates use a separate package and fresh
bottles. The first two patches alone proved insufficient for Wine 11's managed
client-surface lifecycle; gate 3 records the additional fix and runtime proof.

**Provenance.** The pinned revision is
`e94c312f5c054263acf261cfa109edf13e757587`, committed 6 October 2026. Its
`LICENSE` is recorded by size, git blob and SHA-256 in
[runtime-patches](../runtime-patches/README.md); it is a grant notice for
LGPL-2.1-or-later and does not itself contain the licence text, so packaging
must add a full LGPL-2.1 copy, a corresponding-source record and these patches.
Building from that public, separately licensed repository is distinct from
copying anything out of an installed CrossOver, which stays forbidden. This is an
engineering and documentation judgement, not legal advice.

### Historical DXMT gate 2: initial missing Apple component — resolved

The toolchain is now staged inside the ignored build directory: Meson 1.11.2 and
Ninja 1.13.2 in a project-local virtual environment, and LLVM 15.0.7 built from
`llvmorg-15.0.7` for x86_64 with headers and static libraries. Nothing was
installed system-wide.

The cross build configures. Meson selects `x86_64-w64-mingw32-gcc` for the
Windows side and Apple clang for the build side, finds this project's own
`winebuild` through `-Dwine_build_path`, and reports 16 build targets. Two
details were needed beyond the upstream guide: the guide's advice to set
`CFLAGS=-arch x86_64` must not reach the mingw cross compiler, which rejects
`-arch`, and the build-machine side needs a native file forcing x86_64 so the
unixlib can load into the x86_64 Wine runtime on an arm64 Mac. That native file
is kept with the staged build.

Compilation then runs and stops at one external blocker. 17 of 161 steps
complete, including the `winemetal` Windows-side objects, before the first Metal
shader fails:

    xcrun -sdk macosx metal -c src/dxmt/dxmt_command.metal
    error: cannot execute tool 'metal' due to missing Metal Toolchain;
    use: xcodebuild -downloadComponent MetalToolchain

The initial build stopped because the Metal compiler component was missing.
It was subsequently installed through Apple's `xcodebuild -downloadComponent
MetalToolchain`, and both x64 and x86 DXMT builds completed. Reusable build
inputs remain isolated under ignored `.build/`.

### Historical DXMT gate 3: managed Wine 11 surfaces and real Direct3D APIs

The first candidate passed hardware device creation, clear pixels and the
original shader triangle before aborting at Metal-view creation for an HWND.
Correct exports and the window-structure offset were insufficient: Wine 11
creates its client view only when a managed client surface exists. DXMT's old
private-layout fallback never established one.

Two additional original patches give Wine ownership of an opaque Metal client
surface and connect DXMT create/prepare/release to that API. Preparation follows
queued presentation order. Both Windows architectures now pass feature levels
10_0 and 11_0, shader/readback checks and HWND Present calls through Caskrun in
fresh separate bottles. Exact DLL hashes establish translator selection; an
incidental `wined3d` shader-compiler dependency is not mistaken for a fallback.

The [DXMT smoke record](DXMT_SMOKE.md) describes reproduction, lifecycle gates,
source/notice receipts and cleanup. This remains a separate development
candidate. Visible screen composition, real games and safe app-facing runtime
selection/migration remain gates; the current app and user bottles are unchanged.
High Resolution remains optional and off by default.

### Historical WineD3D OpenGL follow-up

The stable independent Wine runtime already loads Apple's OpenGL framework through its x86_64 `winemac.so`; its `SONAME_LIBGL` configure check concerns the X11 GL library and is not the missing macOS graphics path. A fresh disposable bottle using that unchanged runtime reported `GL_VERSION: "4.1 Metal - 90.5"` on Apple M5 Pro. [Apple says OpenGL on Apple Silicon reaches 4.1](https://developer.apple.com/videos/play/wwdc2020/10631/). WineD3D reported a maximum D3D feature level of **9_3 (`0x9300`)**. Its [Wine 11 `feature_level_from_caps()` source](https://gitlab.winehq.org/wine/wine/-/blob/wine-11.0/dlls/wined3d/adapter_gl.c) requires OpenGL 3.2, shader model 4, `ARB_sampler_objects`, and `ARB_polygon_offset_clamp` for level 10_0. The measured extension list contains the sampler extension but not either `ARB_polygon_offset_clamp` or `EXT_polygon_offset_clamp`; Wine's source treats the latter as an alternative spelling. Both supported `WINE_D3D_CONFIG` choices, `shader_backend=glsl` and `shader_backend=glsl-vkd3d`, still reported 9_3 and failed the original 10_0/11_0 device request with `0x80004005`. Thus a clean configure or shader-backend switch cannot raise this Wine/OpenGL combination to D3D 10_0/11_0 on this machine.

A new self-authored x64 D3D11 probe **did create a 9_3 device** (`S_OK`). It cleared a 128×128 RGBA render target, copied it to a staging texture, and read it back with `Map` (`S_OK`); the resulting image was visually checked and all 16,384 pixels were RGB `(13, 173, 242)`. The PNG and PPM are in `.build/wined3d-probe/evidence/` alongside C sources, PE executables, Wine traces, and a `winemac.so` linkage record. A separate triangle trial did **not** render: `CreateVertexShader` returned `0x80070057`, and Wine's trace reported it could not parse the compiler-produced DXBC for the 9_3 device. The clear/readback result proves a narrow D3D11 API operation, not working shaders, D3D10/11 feature levels, a windowed frame, or game compatibility. The 1.7 GB disposable prefix was moved to macOS Trash after retaining evidence; the stable app/runtime were not altered.

The next credible D3D10/11 route remains an independently sourced Metal translator such as DXMT, subject to the build and provenance gates above. WineD3D's 9_3 path can be tested for older titles, but it should not be advertised as modern D3D11 support.

## Historical candidate upstream components

1. **Direct3D 8–11 through DXVK and MoltenVK.** [DXVK](https://github.com/doitsujin/dxvk) translates those Direct3D versions to Vulkan; [MoltenVK](https://github.com/KhronosGroup/MoltenVK) provides a Vulkan portability implementation over Metal on macOS. MoltenVK implements a Vulkan subset, so API coverage and game results must be measured. Caskrun first needs a relocatable x86_64 Vulkan loader/MoltenVK package and a Wine build that can see it, followed by separately sourced DXVK DLLs and per-bottle overrides. Pin source, hashes, notices, and matching architectures before any bundle change.
2. **Direct3D 10/11 through DXMT.** The separately published [DXMT source](https://github.com/3Shain/dxmt) targets Wine on macOS and Metal directly. Its [build guide](https://github.com/3Shain/dxmt/blob/main/docs/DEVELOPMENT.md) currently calls for Meson, LLVM 15, Xcode 16+, Wine 8+, and a Windows cross-compiler for a Wine build. A pinned build with project-local Meson/Ninja and x86_64 LLVM 15 is now packaged separately and passes the original Direct3D API gates described above. App integration and game checks remain separate.
3. **Direct3D 12 is a separate gate.** [vkd3d-proton](https://github.com/HansKristian-Work/vkd3d-proton) is a Vulkan-based candidate, but this Wine/MoltenVK combination has no measured D3D12 support here. Apple's D3DMetal is another possible path only after its distribution terms are reviewed for this open-source project. Do not label either option available in the UI on current evidence.

## Historical validation order

- Build a relocatable x86_64 Vulkan smoke package in ignored `.build/` and prove a Vulkan device is visible from a fresh Caskrun bottle without caller library overrides. Preserve the present WineD3D package until the new one passes.
- Install one independently sourced renderer into a disposable bottle, verify that a self-authored Direct3D test actually uses it, and capture output, version, graphics device, and failures. Test 32-bit and 64-bit paths separately.
- Test one legally obtained game from installation through visible play, exit, and relaunch. Record the exact title/version, macOS/device, store, renderer, settings, and result in a per-game compatibility record.
- Only after that evidence exists, choose a renderer automatically for the tested title and keep overrides under bottle Advanced settings. Do not make the person choose Wine, Vulkan, or a renderer in the normal Install → Open → Play flow.

This graphics work is required before Caskrun can approach CrossOver-style game compatibility. Every renderer and game needs its own evidence and license review; a launcher opening is not a game test.

### Historical optional app integration — 8 October 2026

The development app now offers Automatic (WineD3D), WineD3D and verified DXMT
in the bottle inspector. A switchable package retains the original default
runtime and selects a separate real DXMT view; its exact package identity gates
capabilities and a reviewed idle native-v2 upgrade. Selection, native identities,
shutdown proofs and pre-Wine record rollback are bottle-scoped. See
[app integration](DXMT_APP_INTEGRATION.md). Visible output and real games remain
unvalidated; other backends and synchronization remain unavailable. High
Resolution remains optional and off by default.

### Historical optional DXMT presentation controls

The bottle inspector also provides an application-controlled/default frame
target or a bounded FPS target and optional 1.25×, 1.5× or 2× MetalFX spatial
output scale. Both controls default off and require verified DXMT, a capable
device where relevant, and stopped bottle apps before changes. They preserve
the default renderer and require no new runtime bytes.

The original live matrix passed all five tested control combinations under both
Windows x64 and x86, including drawing/readback, two-window resize/recreation,
runtime ownership, configuration precedence, refused edits while running and
owned cleanup. API-submission timing does not measure displayed game FPS.
Visible composition, image quality, fullscreen behavior, performance and games
remain manual gates. DLSS, temporal reconstruction, frame generation, DXVK,
D3DMetal, D3D12 and MSync are not supplied by these controls. See
[DXMT options](DXMT_OPTIONS.md) for exact settings and reproduction.
