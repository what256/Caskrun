# Graphics compatibility path

_Research snapshot: 23 September 2026. This is an implementation plan, not a claim that a Windows game renders in Caskrun._

Caskrun currently bundles independently built Wine 11.0 and uses Wine's default graphics path. Local Wine logs report missing Vulkan support; no D3D game, controller, frame rate, or graphics switch has passed a Caskrun game test. A successful Steam updater does not establish graphics compatibility.

An [isolated full Wine/Vulkan candidate](VULKAN_CANDIDATE.md) has since passed fresh-bottle x64 and x86 Vulkan device enumeration after relocation and removal of its build tree. It remains outside the app and DMG. Direct3D translation and game rendering remain unverified, so the current bundled WineD3D path is unchanged.

## Isolated Vulkan and Direct3D probe, 23 September 2026

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

### Integration contract for the next renderer experiment

1. Rebuild the **whole** independent Wine runtime against pinned x86_64 Vulkan Loader and MoltenVK development files. Stage matching x86_64 Unix modules and Windows PE DLLs, and preserve Wine's separate 32-bit PE path. Do not graft the prototype's two modules into a shipped Wine build without a full rebuild and smoke suite.
2. Put `libvulkan.1.dylib`, `libMoltenVK.dylib`, and its unchanged portability ICD JSON inside the runtime. Resolve their absolute paths from the runtime's own location when launching, then set `DYLD_FALLBACK_LIBRARY_PATH` and `VK_DRIVER_FILES` only inside the Wine entrypoint. The successful probe used no caller graphics environment overrides. Verify Mach-O architectures, deployment target, linkage, relocation, signatures, hashes, and license notices after packaging.
3. Choose a renderer only after a self-authored Direct3D test creates a device, draws a frame, and produces a visible result in a fresh bottle. Verify x64 and x86 separately; record which DLL actually loaded and its renderer log. Then test a real game through install, launch, play, exit, and relaunch before assigning a default for that title. Leave the ordinary Install → Play flow free of Wine/Vulkan choices.

Existing initialized bottles are bound to the exact bundled runtime-tree digest in `core/src/main.rs`. The only current migration path in `core/src/runtime_migration.rs` is a narrowly approved transition for an idle, verified Steam bottle. A Vulkan candidate must therefore use fresh disposable bottles first; changing the development DMG alone would not make it safe to switch existing bottles. Before enabling per-bottle renderer selection for existing users, add a general idle-bottle migration with an exact source/destination identity check and a verified rollback path.

The separate [DXMT upstream build guide](https://github.com/3Shain/dxmt/blob/main/docs/DEVELOPMENT.md) is a plausible next D3D10/11 experiment because it targets Metal directly and avoids this Vulkan device limit. Its documented Wine cross-build requires **Meson 1.3+**, **LLVM 15 exactly with x86_64 headers and static libraries**, Xcode 16+, Wine 8+ build files, and a Windows cross-compiler. This Mac has Xcode 26.6, Wine 11 build files, MinGW, and modern CMake, but no staged Meson, Ninja, or x86_64 LLVM 15; no DXMT build or D3D smoke was performed. The separately published [DXMT license](https://github.com/3Shain/dxmt/blob/main/LICENSE) is LGPL-2.1-or-later and credits Feifan He **for CodeWeavers**. Review that upstream provenance against the project's no-CrossOver-code requirement before including it. If approved, pin a DXMT source revision and toolchain, build x64 PE DLLs plus its x64 Unix library in ignored `.build/`, and run the same device/frame/visual tests; 32-bit PE DLLs are a separate build and depend on the x64 Unix library.

### DXMT gate 1, measured: the engine hides what it must export — 8 October 2026

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

Both patches are recorded in [runtime-patches](../runtime-patches/README.md) and
neither is packaged: including the Wine one rebuilds the engine and changes the
runtime-tree digest existing bottles are bound to.

Whether `dlsym(RTLD_DEFAULT, ...)` can then reach those symbols from DXMT's own
unix library is a second, untested question, because it depends on how Wine loads
driver libraries. Nothing has been built or executed on the DXMT side.

**Provenance.** The pinned revision is
`e94c312f5c054263acf261cfa109edf13e757587`, committed 6 October 2026. Its
`LICENSE` is recorded by size, git blob and SHA-256 in
[runtime-patches](../runtime-patches/README.md); it is a grant notice for
LGPL-2.1-or-later and does not itself contain the licence text, so packaging
must add a full LGPL-2.1 copy, a corresponding-source record and these patches.
Building from that public, separately licensed repository is distinct from
copying anything out of an installed CrossOver, which stays forbidden. This is an
engineering and documentation judgement, not legal advice.

### DXMT gate 2: the build runs and stops at one missing Apple component

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

The `metal` driver exists in Xcode 26.6 but its toolchain component is not
installed. Installing it changes the Xcode installation on the development Mac,
so it is left to the project owner. No source or build problem is known beyond
this point; the remaining steps have not been attempted.

**Still untested.** Nothing has been loaded into a bottle, no Direct3D call has
been made through DXMT, and no rendering check has run. Those need the engine
rebuilt with the visibility patch and repackaged behind the idle-bottle migration
path, which has not been done.

### WineD3D OpenGL follow-up

The stable independent Wine runtime already loads Apple's OpenGL framework through its x86_64 `winemac.so`; its `SONAME_LIBGL` configure check concerns the X11 GL library and is not the missing macOS graphics path. A fresh disposable bottle using that unchanged runtime reported `GL_VERSION: "4.1 Metal - 90.5"` on Apple M5 Pro. [Apple says OpenGL on Apple Silicon reaches 4.1](https://developer.apple.com/videos/play/wwdc2020/10631/). WineD3D reported a maximum D3D feature level of **9_3 (`0x9300`)**. Its [Wine 11 `feature_level_from_caps()` source](https://gitlab.winehq.org/wine/wine/-/blob/wine-11.0/dlls/wined3d/adapter_gl.c) requires OpenGL 3.2, shader model 4, `ARB_sampler_objects`, and `ARB_polygon_offset_clamp` for level 10_0. The measured extension list contains the sampler extension but not either `ARB_polygon_offset_clamp` or `EXT_polygon_offset_clamp`; Wine's source treats the latter as an alternative spelling. Both supported `WINE_D3D_CONFIG` choices, `shader_backend=glsl` and `shader_backend=glsl-vkd3d`, still reported 9_3 and failed the original 10_0/11_0 device request with `0x80004005`. Thus a clean configure or shader-backend switch cannot raise this Wine/OpenGL combination to D3D 10_0/11_0 on this machine.

A new self-authored x64 D3D11 probe **did create a 9_3 device** (`S_OK`). It cleared a 128×128 RGBA render target, copied it to a staging texture, and read it back with `Map` (`S_OK`); the resulting image was visually checked and all 16,384 pixels were RGB `(13, 173, 242)`. The PNG and PPM are in `.build/wined3d-probe/evidence/` alongside C sources, PE executables, Wine traces, and a `winemac.so` linkage record. A separate triangle trial did **not** render: `CreateVertexShader` returned `0x80070057`, and Wine's trace reported it could not parse the compiler-produced DXBC for the 9_3 device. The clear/readback result proves a narrow D3D11 API operation, not working shaders, D3D10/11 feature levels, a windowed frame, or game compatibility. The 1.7 GB disposable prefix was moved to macOS Trash after retaining evidence; the stable app/runtime were not altered.

The next credible D3D10/11 route remains an independently sourced Metal translator such as DXMT, subject to the build and provenance gates above. WineD3D's 9_3 path can be tested for older titles, but it should not be advertised as modern D3D11 support.

## Candidate upstream components

1. **Direct3D 8–11 through DXVK and MoltenVK.** [DXVK](https://github.com/doitsujin/dxvk) translates those Direct3D versions to Vulkan; [MoltenVK](https://github.com/KhronosGroup/MoltenVK) provides a Vulkan portability implementation over Metal on macOS. MoltenVK implements a Vulkan subset, so API coverage and game results must be measured. Caskrun first needs a relocatable x86_64 Vulkan loader/MoltenVK package and a Wine build that can see it, followed by separately sourced DXVK DLLs and per-bottle overrides. Pin source, hashes, notices, and matching architectures before any bundle change.
2. **Direct3D 10/11 through DXMT.** The separately published [DXMT source](https://github.com/3Shain/dxmt) targets Wine on macOS and Metal directly. Its [build guide](https://github.com/3Shain/dxmt/blob/main/docs/DEVELOPMENT.md) currently calls for Meson, LLVM 15, Xcode 16+, Wine 8+, and a Windows cross-compiler for a Wine build. This Mac has Xcode 26.6 and MinGW; Meson and a matching project-local LLVM 15 are not staged. Its upstream source and license would need an explicit provenance review against the project's no-CrossOver-bundle rule before inclusion.
3. **Direct3D 12 is a separate gate.** [vkd3d-proton](https://github.com/HansKristian-Work/vkd3d-proton) is a Vulkan-based candidate, but this Wine/MoltenVK combination has no measured D3D12 support here. Apple's D3DMetal is another possible path only after its distribution terms are reviewed for this open-source project. Do not label either option available in the UI on current evidence.

## Validation order

- Build a relocatable x86_64 Vulkan smoke package in ignored `.build/` and prove a Vulkan device is visible from a fresh Caskrun bottle without caller library overrides. Preserve the present WineD3D package until the new one passes.
- Install one independently sourced renderer into a disposable bottle, verify that a self-authored Direct3D test actually uses it, and capture output, version, graphics device, and failures. Test 32-bit and 64-bit paths separately.
- Test one legally obtained game from installation through visible play, exit, and relaunch. Record the exact title/version, macOS/device, store, renderer, settings, and result in a per-game compatibility record.
- Only after that evidence exists, choose a renderer automatically for the tested title and keep overrides under bottle Advanced settings. Do not make the person choose Wine, Vulkan, or a renderer in the normal Install → Open → Play flow.

This graphics work is required before Caskrun can approach CrossOver-style game compatibility. Every renderer and game needs its own evidence and license review; a launcher opening is not a game test.
