# AnyPS5 through Wine: development investigation

_9 October 2026. Experimental work; PS5 gameplay on macOS is not validated._

AnyPS5 converts a PS5 executable into a Windows executable and supplies replacement
system libraries. Wine then runs the Windows output. Sharing a Unix ancestry does
not make PS5 executables native Mac programs: the CPU instruction set, executable
format, system interfaces and graphics API still need their respective bridges.
This investigation does not add AnyPS5 to Caskrun's engine chooser.

## Tested inputs

- Official [AnyPS5 v0.1.1](https://github.com/boykopovar/AnyPS5/releases/tag/v0.1.1),
  source revision `fe1935ef682d0b6fb94c0a3554a531b695302a8c`.
  Converter SHA-256:
  `1553f254a74f10b9c6bdd97934647709a1171dec9de15248455f827f0c174521`.
  Windows runtime archive SHA-256:
  `e1cae91e9cd846f5d5670b4ebe20f6d002d75efb974bb0cb64cec4e0b507f0c5`.
- The current independent DXMT-capable Caskrun runtime, tested in a new bottle.
- The separate [Wine Vulkan candidate](VULKAN_CANDIDATE.md), containing Wine 11.0,
  Vulkan Loader 1.4.350 and official MoltenVK 1.4.2, tested in another new bottle.
- A small GPL-2.0 upstream ELF fixture with no guest imports. Its relocated entry
  point returns 42. Fixture SHA-256:
  `c90f460fd4029a2deb3d8508d9ff9ad01d51d0adfca71f7c1b045135b323d9d1`.
- Public [PS5 DOOM homebrew v1.1.1](https://github.com/lowbit/ps5-doom/releases/tag/v1.1.1).
  Its package includes the original shareware episode and license notices, rather
  than a retail PS5 dump. Archive SHA-256:
  `4d8e5b785b61696fbf1988c2d0d87c5c977734ff60b1ce11cc49134b0e711b5f`.
  No homebrew executable, game data or downloaded runtime is included in Git.

Downloads were checked against the published release hashes. ZIP entries were
checked for traversal, absolute names, symlinks and bounded extracted sizes.
DOOM's development containers were unwrapped using a bounded tool reviewed
against its own pinned public packaging source. Both reconstructed ELF files
matched the containers' embedded SHA-256 values. Encrypted segments were refused;
no decryption keys were used. The original package was preserved.

## Observed results

| Check | Result | Scope |
| --- | --- | --- |
| Official Windows converter under current Wine | Converted the tiny fixture successfully | Converter execution |
| Converted fixture under current Wine | Process exited with the expected code 42 | Simple guest relocation and execution |
| Newer converter's tiny Windows output under candidate Wine | Process exited with the expected code 42 | Newer generated startup and guest relocation |
| Current Wine Vulkan initialization | `vkCreateInstance` returned `-3`, no instance extensions | Current engine cannot initialize this Vulkan path |
| Vulkan candidate under Wine | Instance, physical device, minimal graphics device and queue created; exit 0 | Initialization only |
| Native macOS comparison through the same loader and driver | Same successful initialization and queried feature values; exit 0 | Separates driver capability from Wine reporting |
| DOOM homebrew with the v0.1.1 converter | Conversion failed before producing a Windows executable | No game launch or gameplay result |
| DOOM homebrew with native converter snapshot `8b393b6` | Converted successfully with ordinary bundled-module processing and syscall checks | Windows PE creation |
| Newer DOOM output with v0.1.1 libraries under candidate Wine | Loaded its libraries, then failed at an unresolved kernel import; exit 29 | Explicitly unsupported version mix, startup failed before game entry |

The DOOM conversion returned exit 2 with `Instruction truncated before ModRM
byte`. It used the unwrapped executable and bundled module, with normal module
processing enabled. This is a reproducible converter failure, not evidence about
DOOM's performance, rendering or input under Wine.

The failure originated in the bundled homebrew `libc` shim, rather than the
game's main executable. Its executable segment ends with an odd-length zero
tail. A separate release converter candidate incorporating upstream's
[`f4f8d5b` zero-tail fix](https://github.com/boykopovar/AnyPS5/commit/f4f8d5bbce747fefdc583d15948fd9ccd9ca6977)
passed that point, then found conflicting symbol-alias ranges. Upstream's
[`93efde8` alias-range fix](https://github.com/boykopovar/AnyPS5/commit/93efde8056d6b323935739a00ad2d8428a1073d9)
resolved that conflict, but the old collector then rejected overlapping
instruction boundaries in the same shim. These were distinct converter
analysis failures; a two-patch release converter was not a working solution.
The original release source was preserved.

The documented `ANYPS5_RELINKER_ONLY=ON` build at snapshot `8b393b6` produced a
native ARM64 converter. It converted the unwrapped DOOM executable and module
without disabling validation or excluding modules. The generated Windows PE
SHA-256 is `ff3a0602b82285abf9e3863583f82c51980c1c009d30f707ec1efaee6adc5a01`.
Converter regressions covered zero padding versus malformed nonzero tails,
forbidden syscall opcodes, alias extents, segment bounds, conflicting unwind
ranges, TLS handling and Windows output inspection. These do not establish a
compatible Windows runtime or a working game.

Before the experimental startup attempt, upstream's PE import audit examined
171 main-executable references representing 170 distinct imports against the
official release libraries and their corresponding source. It classified 144
as implemented, 12 as throwing stubs and 14 as absent from the host libraries.
The generated guest library exports six of those absent libc identifiers; eight
remain absent: two HTTP functions, one user-service function and five kernel
functions. There were no missing library files or library-name mismatches.
The audit's implemented category is a source/export classification, not proof
of correct behavior; it does not validate guest shim bodies or all guest imports.

A bounded Wine attempt with the successful newer conversion and official older
libraries confirmed a loader failure at an unresolved `libkernel.prx` import.
It exited with code 29 and an entry-point-not-found exception before reaching
the game's entry point. The test did not produce gameplay or establish whether
the older renderer could present this game. This mixed package is not installed
in Caskrun or represented as a supported engine.

The original diagnostic sources and reproduction instructions are in
[`scripts/diagnostics/anyps5-vulkan/`](../scripts/diagnostics/anyps5-vulkan/).
Capability enumeration and a device with no optional features do not prove that
AnyPS5 can create its complete game device, present a frame or run a game.

## Release and newer-code requirements differ

The candidate reports the examined v0.1.1 graphics requirements available:
Vulkan 1.1, surface/swapchain extensions, eight-bit storage, buffer device
addresses, shader 64-bit integers, the required float-control property,
vertex/fragment storage operations, anisotropic sampling and BC compression.
This does not validate every shader, texture format or game-specific feature.

The examined newer AnyPS5 source at
`8b393b6e9a8ade9188d4f74a0293e0efd9828597` additionally
[requires and enables `nullDescriptor`](https://github.com/boykopovar/AnyPS5/blob/8b393b6e9a8ade9188d4f74a0293e0efd9828597/core/libs/prx/libSceAgcDriver/Execution/src/VulkanDevice.cpp#L1116-L1128).
Both native and Wine probes report `nullDescriptor = false`.
[MoltenVK 1.4.2 explicitly reports that feature as unsupported](https://github.com/KhronosGroup/MoltenVK/blob/v1.4.2/MoltenVK/MoltenVK/GPUObjects/MVKDevice.mm#L611-L617).
The extension name is advertised; that alone does not mean every feature in the
extension is supported. This is not a Wine feature-query defect or a hidden
graphics preference.

DOOM's CPU scaler does not remove this constraint. The game opens VideoOut before
choosing its scaler, and AnyPS5 constructs its Vulkan device for presentation.
The scaler avoids the game's compute shader, not the engine's presentation
device requirements.

## Work needed before integration

1. Keep the successful, pinned newer converter build and its regression evidence.
   The official release converter remains unsuitable for this homebrew input.
2. Use a matched converter and Windows system-library build. The official release
   predates later homebrew fixes; mixing newer conversion behavior with older
   libraries must not be treated as a supported engine package.
3. Implement and verify a real fallback for unsupported null descriptors, or use
   a driver that implements the feature. Removing the guard or claiming support
   through a forced flag is not a fix.
4. Validate a real game: startup, presentation, input, sound, saves, shutdown and
   relaunch. Only then expose the validated engine route in Caskrun.

The documented Windows build uses GCC 15.2 UCRT. The available Mac cross-compiler
uses GCC 16.1 with a different CRT baseline, and the full build executes generated
Windows host tools. A full runtime cross-build needs an explicit host/target tool
split and matching dependencies; a native Mac converter build alone does not
produce the Windows system libraries.

All experiment data stays under ignored `.build/anyps5-wine-oct09/`. Each test uses a
separate Caskrun data directory, bottle and runtime binding. Test profile host
folder links are replaced with empty private folders and the host-root drive
mapping is removed before test executables run. Both owned prefixes were stopped;
process-environment and open-file checks found zero remaining owners before the
two test bottles were removed. Raw session reports and compilation intermediates
were removed. Reviewed public source, verified downloads, the successful native
converter and sanitized capability evidence remain in the ignored experiment
folder. The
installed app, user collection, existing bottles and active runtime are not
modified by this investigation.
