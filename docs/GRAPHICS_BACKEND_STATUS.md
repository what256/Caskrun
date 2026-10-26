# Graphics backend status

_Source and upstream review: 26 October 2026. This records implementation
boundaries; it does not declare untested engines or games compatible._

## Completion-pass validation

Build 56 completes tuning for Automatic/DXMT without changing the selected
Automatic preference or its cached decision. EA's versioned launcher paths now
retain startup feedback, and packaging refuses replacement while Core or the
icon helper is in use. The layout and included V14 runtime remain unchanged.

The final serial Rust run passed 639 unit and 7 integration tests; 3 existing
unit tests remain ignored. Focused coverage includes 35 graphics tests,
29 Swift preference checks, 128 Swift startup checks and 42 Python checks.
The full app compiled, its deep signature and development disk image verified,
and the packaged Core matched the final production source after removing only
its signature for comparison. Clippy reported no new graphics warnings.

Four initial failures were bounded mock-startup waits that expired before fresh
shell helpers ran. The fixture waits now allow measured startup latency while
retaining their exact command, owner and prefix assertions; production waits
are unchanged. One post-reap ownership check passed repeated unchanged runs
and the final full run. Its strict refusal remains intact.

No real Wine runtime, storefront or game was launched in this pass. Game and
publisher acceptance remains for the user's [manual tests](NEXT_MANUAL_TESTS.md).

## Included capabilities

| Capability | Included behavior | Remaining acceptance |
| --- | --- | --- |
| WineD3D | Existing default/explicit fallback for older graphics paths. The original OpenGL experiment measured feature level 9_3. | Per-title rendering and performance. It is not a modern D3D11 feature-level replacement. |
| DXMT | Verified optional Direct3D 10/11 Metal view, x86/x64 API fixtures, idle-only selection and saved preferences. | Sustained play, image quality, performance and devices beyond the recorded results. |
| Automatic | Bounded PE import inspection can select DXMT for supported x64 Direct3D 10/11 evidence at an idle launch. Unknown/mixed evidence retains WineD3D; manual selection remains explicit. | Static imports cannot identify every dynamically loaded graphics API. |
| DXMT frame target / MetalFX spatial scaling | Typed optional tuning, saved per bottle; original rendering and configuration tests recorded separately. | Real displayed FPS, fullscreen behavior and visual quality. Spatial scaling does not supply DLSS, temporal reconstruction or frame generation. |
| High Resolution Mode | Optional Retina backing store and 96/192 DPI setting with idle checks and rollback. | DPI-unaware applications can still render at 1x. Existing choices are preserved. |
| DXVK | Not integrated. The earlier independent MoltenVK experiment enumerated Vulkan devices but failed the Direct3D device gate. | A new complete Vulkan runtime and a verified translator/device/frame path. |
| D3DMetal | Not integrated or bundled. | Official component, applicable Apple terms, host-interface integration and independent runtime tests. |
| MSync | Not implemented in the current engine. | A coordinated upstream Wine 11 port and synchronization validation; see [MSync feasibility](MSYNC_FEASIBILITY.md). |
| Linux ntsync / fsync | No corresponding macOS implementation in the included engine. | Linux kernel interfaces cannot be enabled by a macOS environment toggle. |
| D3D12 / temporal scaling / frame generation | Not supplied by the reviewed package. | Separate complete implementation, component provenance and API/game tests. |

The pinned DXMT source contains an experimental `enable_d3d12` option, but the
reviewed package disables it and the retained source still has unimplemented
D3D12 methods. Merely compiling that option or displaying a picker would not
establish a working DirectX 12 backend. The same package disables NVAPI and
NVNGX; it has no validated vendor-extension route for temporal reconstruction.

## A newer independent Vulkan route

The older [Vulkan candidate](VULKAN_CANDIDATE.md) measured a MoltenVK device
below the requirements of current DXVK. Current primary sources now describe
another candidate:

- [Khronos' MoltenVK repository](https://github.com/KhronosGroup/MoltenVK/blob/main/README.md)
  recommends KosmicKrisp for Apple Silicon and marks MoltenVK community maintained.
- [Mesa's KosmicKrisp documentation](https://docs.mesa3d.org/drivers/kosmickrisp.html)
  describes a Vulkan-conformant Metal 4 driver requiring macOS 26 or later.
- [LunarG's October 2026 overview](https://www.lunarg.com/the-state-of-vulkan-on-apple-oct-2026/)
  records Vulkan 1.4 conformance and recommends bundling a tested loader and
  driver instead of relying on a system installation.

Caskrun's Unix Wine modules run as x86_64 under Rosetta, so an ARM64-only driver
cannot simply be loaded into that process. There is source evidence that an
x86_64 build is feasible: Mesa documents a
[Rosetta-specific heap workaround](https://docs.mesa3d.org/drivers/kosmickrisp/workarounds.html#kk-workaround-8),
and the separate [shadexternals build project](https://github.com/shadexternals/mesa-kosmickrisp)
publishes an x86_64 cross-file and an ARM-host cross-build recipe. That recipe
is evidence about building, not proof of a reviewed Caskrun artifact. No driver
binary was loaded in this phase.

Official [SDK metadata](https://vulkan.lunarg.com/sdk/files.json) lists:

| Artifact | Version | Compressed bytes | SHA-256 |
| --- | --- | ---: | --- |
| `vulkansdk-macos-1.4.363.0.zip` | 1.4.363.0 | 396088200 | `163431e0203c8215286a17cf393643ce6b951f77d1b4f74ab30821a1f0beb09b` |

No separate driver artifact is listed there. Bounded HEAD/range requests to the
official SDK download endpoints returned HTTP 403 during this review. The full
installer was not downloaded, installed or run, and no Mach-O slice/signature
or component-license receipt was established. SDK metadata is a pin for future
review, not approval to include the package.

### Required integration gates

1. Obtain or build an independently pinned x86_64 KosmicKrisp, matching Vulkan
   loader and headers. Audit actual architectures, deployment targets, linkage,
   relocation, hashes and licenses. Mesa's driver is MIT licensed; the full
   dependency set needs its own notices and source records.
2. Build the complete independent Wine Vulkan modules for the current Wine
   source and original patches, including x86 and x64 Windows paths. Keep the
   existing DXMT/WineD3D package intact while preparing the candidate.
3. Query the actual driver requirements before selecting a translator.
   [DXVK's driver requirements](https://github.com/doitsujin/dxvk/wiki/Driver-support)
   include Vulkan 1.4 and specific extensions/features; Vulkan conformance
   alone does not prove every optional requirement is available.
4. Pass original x86/x64 Direct3D device, shader/readback, presentation and
   lifecycle tests with mapped-library evidence, then add exact capabilities
   and idle-only runtime migration. Games remain a separate user test.

This is a credible new research path, not an included DXVK implementation.
No store, game, driver service or background Windows process was started for
this source review.

## D3DMetal component boundary

[Apple's Game Porting Toolkit page](https://developer.apple.com/games/game-porting-toolkit/)
describes its Windows-game evaluation environment. Its D3DMetal binary is a
separate component whose applicable download terms, integrity and integration
must be reviewed before use. The Apache license on Apple's separate porting
sample/agent repository does not license that binary.

Caskrun currently has no reviewed D3DMetal payload or host-interface path.
Neither another app's files nor an unchecked download should be treated as an
engine update. A future official component import must preserve its supplied
license and integrity, use an explicit compatible runtime view, and remain
outside public artifacts until its distribution review is complete.
