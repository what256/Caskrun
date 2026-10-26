# Graphics backend status

_26 October 2026. Current source and isolated engine probes are recorded below.
The 0.2.0 development app adds the reviewed V16 DXMT D3D12 component; no
storefront or game was launched in this engine pass._

## Current engine work

| Capability | Source and candidate evidence | Remaining acceptance |
| --- | --- | --- |
| WineD3D | Existing explicit fallback and conservative Automatic default. The historical OpenGL probe reached feature level 9_3. | Per-title rendering and performance; it is not a modern D3D11 feature-level replacement. |
| DXMT | Optional Direct3D 10/11 Metal view and experimental x64 Direct3D 12 in V16. Normal launches and read-only Steam controls share saved selection/tuning. Frame target now also forwards D3D12's setting; spatial scaling remains D3D10/11 only. | Sustained play, visible image quality, performance and additional devices. |
| DXVK hybrid | Candidate combines source-built legacy D3D9 with modern DX10/11 and official MoltenVK. The complete x86/x64 D3D9+D3D11 GPU matrix passed with exact mapped-library evidence; see below. | Store/game testing and performance. D3D8 stays on its original Wine implementation; D3D12 is not supplied. |
| Automatic | Bounded complete PE import evidence can select verified DXMT for supported x64 Direct3D 10/11 or the verified hybrid DXVK view for unmixed x86/x64 Direct3D 9. Mixed, unknown or incomplete evidence retains WineD3D. Manual choices remain explicit. | Static imports cannot identify every dynamically loaded API or determine game compatibility. |
| MSync | Original Wine 11 port of the public generic implementation, coordinated client/server protocol 932. Both retained WineD3D/DXMT views passed enabled/disabled x86/x64 synchronization probes. Opt-in, off by default. | Real-game behavior and measured performance; unsupported mixed wait objects remain a compatibility limit. |
| High Resolution Mode | Existing optional Retina backing store and 96/192 DPI settings, with idle checks and rollback. | DPI-unaware apps can remain upscaled. Existing choices and defaults stay unchanged. |
| D3DMetal | Official input passed a separate native host factory/device/256-pixel clear control. The Wine-host factory gate still fails; it is not selectable or included in the shared engine. | A compatible Wine host-interface path and successful Windows API/lifecycle tests; distribution review remains separate. |
| D3D12 | Included in the exact V16 package under manual DXMT. Actual Core/dispatcher passed shader draw/readback, presentation/resize/recreation, idle V15 upgrade with saved 60 FPS preference, busy refusal and switchback. Automatic remains conservative. | Game/image-quality/performance tests; this experimental pin lacks DXIL and many modern D3D12 features. See [integration](D3D12_EXPERIMENT.md). |
| Temporal scaling / frame generation | Not supplied by these paths. DXMT spatial scaling and frame targets do not add DLSS, temporal reconstruction or frame generation. | Separate implementation, provenance and API/game tests. |
| Linux ntsync / fsync | No matching macOS kernel backend is supplied. | Linux kernel interfaces cannot be enabled by an environment toggle on macOS. |

The existing Collection inspector layout is retained. DXVK appears only with
the matching reviewed package; MSync gets a small bottle-scoped toggle only
when supported or already saved. Selection and synchronization changes require
the bottle lock and idle/owner checks. Version-3 preferences preserve the
renderer, DXMT tuning and MSync through duplicate and backup/restore without
carrying runtime paths. Unavailable saved choices fail clearly rather than
silently choosing a different engine. Diagnostic identities and DXMT capability
now use the same reviewed package allowlists as runtime selection.

## Hybrid DXVK GPU evidence

The candidate uses two separately pinned Zlib-licensed implementations:

- [Metalsharp DXVK-MacOS](https://github.com/metalsharp/DXVK-MacOS/tree/bd8921a3db6c4928f5b34672791121f7c14e38c2),
  version 3.1-macos1.0, for D3D10/11.
- [tairasu D9VK](https://github.com/tairasu/d9vk/tree/7437b616f05bc678d2ff4b4b75832f9cbddeed75),
  version 1.10.3-20230507-async-macos, built from source for D3D9.
- Official [MoltenVK 1.4.2](https://github.com/KhronosGroup/MoltenVK/tree/db66022459ffb663aa2b50f6b018bc2e124f5edf),
  under Apache-2.0, from the pinned official Vulkan SDK. Loader, Wine source,
  notices and portability patch are recorded by the candidate packager.

| Original probe | Per-architecture result, both x64 and x86 WoW64 |
| --- | --- |
| D3D11 | 242 checks, 56 readback pixels, 14 Present calls; hardware feature levels 10/11, shader/readback, multiple windows, resize, destroyed-window handling and recreation. |
| D3D9 | 154 checks, 24 exact pixels, 6 Present calls; hardware fixed-function triangle/readback and Reset/resize. |

One coherent hybrid view passed all four cells. Each owned process mapped the
selected PE files, the reviewed MoltenVK and Vulkan loader; no alternate
renderer was present at the pre-compiler identity gate. All ten graphics DLLs
inside the probe prefix and all eight selected view PEs remained unchanged.
Wine's supported builtin DOS-stub marker changes only the reserved marker
bytes; original and runtime hashes are recorded. No prefix/application graphics
DLL is replaced. Every renderer view routes to one common wineserver.

The modern D3D9 fixed-function path failed despite successful device/clear
checks; that failure is why the candidate uses the separately pinned legacy
D3D9 implementation. Missing geometry/transform-feedback and robustness
features still limit titles. The probes do not prove general DXVK, D3D8,
D3D12, game or performance compatibility. See [Vulkan evidence](VULKAN_CANDIDATE.md).

## MSync evidence

The matching client/server port passed 37 synchronization assertions in each
of eight architecture/mode/view cells: 296 checks across x86/x64, MSync off/on,
and WineD3D/DXMT. Coverage includes cross-process wakeups, event/semaphore/mutex
semantics, waits, timeouts, APC cancellation, exit, access checks and handle
reuse. The ordinary server fallback remains available. These results are API
evidence, not a gameplay or speed claim. See [MSync implementation and limits](MSYNC_FEASIBILITY.md).

## D3DMetal boundary

[Apple's evaluation component](https://developer.apple.com/games/game-porting-toolkit/)
is separately licensed. Its local inspection/staging tool records the official
input and supplied license without copying another app's Wine engine or
altering Apple's component. The actual Apple signature was retained. The
factory failure occurs before a working device/frame path; whether the
independent Wine host ABI can satisfy it is unresolved. The private payload,
account/download data and raw diagnostics are excluded from source/public
artifacts. License acceptance for local evaluation is not redistribution
approval. No D3DMetal capability is advertised from a successful file import.

A subsequent basic native host control passed factory/device creation and
256/256 clear/readback pixels without modifying Apple's component. This
isolates framework health from the still-unresolved Wine integration. A
separate DXMT D3D12 development route also passed hardware clear and shader
draw/readback in Wine. Neither result changes build 57's capabilities. See the
[D3D12 development record](D3D12_EXPERIMENT.md) for source pins and limits.

## Local build 57 acceptance

The final reviewed V15 package is
`sha256:2850553486175ba0326c5d4f84ea45f3fb37a5210c6830e77d7bea5ca8f97a0e`.
The actual Core-selected DXVK four-cell matrix passed through the packaged
dispatcher. DXMT also passed both architecture lifecycle matrices, return to
WineD3D/Automatic, busy-switch refusal and owned-session cleanup. The final
MSync enabled/disabled WineD3D/DXMT matrix passed all 296 assertions. The final
notice supplement changed only six license files; all tested binaries stayed
byte-for-byte unchanged.

The complete serial Rust run passed 659 unit and 7 integration tests, with
3 existing tests ignored. After enabling upstream ARM hardware SHA-256,
four focused tamper/cache/relocation tests and an independent complete-package
identity comparison passed. Full engine verification measured 12.30 seconds
with software SHA and 4.38 seconds with hardware SHA on the same local input;
this does not measure game performance or remove any verification step.
Swift preference contracts passed 35 checks; dispatcher, package boundaries,
GPU proof parsing, DXMT harness, runtime-package and Apple private-import
boundaries passed their Python groups. The app compiled, its signatures and
runtime copy were verified, and the development image passed disk-image
verification. Only source is eligible for the separately checked Git upload.
Apple's evaluation payload is absent from the app and development image.

## Historical validation

Build 56/V14 preceded this engine work. It completed Automatic/DXMT tuning,
publisher startup feedback and idle packaging checks without changing its
runtime. Its serial run passed 639 Rust unit and 7 integration tests, with
3 existing unit tests ignored; focused results included 35 graphics,
29 Swift preference, 128 Swift startup and 42 Python checks. Its app signature,
development image and packaged Core were checked. These are historical build
results, not acceptance of the new candidate.

The September independent MoltenVK candidate enumerated devices but did not
establish a Direct3D path. The earlier stock DXVK/MoltenVK combination failed
its device requirements. KosmicKrisp remains a separate experimental candidate;
it is not the driver used for the successful hybrid results above. Earlier
metadata/download and source-feasibility observations do not override actual
mapped-library/render evidence.

Real publisher sign-in, install/play/relaunch, fullscreen, input, sound and
performance remain for the user's [manual checks](NEXT_MANUAL_TESTS.md). No
claim is made that all graphics work or all launchers are finished.
