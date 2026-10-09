# AnyPS5 through Wine: development investigation

_10 October 2026. PS5 DOOM homebrew ran experimentally through Wine on Apple
Silicon. Retail PS5 compatibility is not validated._

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

DOOM's CPU scaler does not remove this constraint in unmodified AnyPS5. The
game opens VideoOut before choosing its scaler, and AnyPS5 constructs its
Vulkan device for presentation. The scaler avoids the game's compute shader,
not the engine's presentation device requirements. The limited capability
patch described below makes the no-image-resource path eligible without
claiming null-descriptor support.

## Matched runtime follow-up

The separate experimental build now uses the same `8b393b6` source revision
as the successful converter, all eleven exact submodule revisions, and the
upstream-supported Windows GCC 15.2.0 UCRT compiler. Running that compiler in
an isolated Wine prefix preserves upstream's DWARF unwind requirements. A
small compiler fixture confirmed that the available Mac GCC 16.1 cross-compiler
instead produces SjLj exception calls with the upstream flags; simply changing
those flags would not establish a compatible runtime.

All 116 NID-patched system libraries built. An export audit against the main
DOOM import registry found 171 references / 170 distinct imports, zero absent
imports, zero missing libraries and zero library-name mismatches. A separate
audit of the actual generated PE bootstrap found 274 references / 262 distinct
imports including the guest libc module; each has an allowed provider. Recursive
PE import/export verification covered 150 images and 14,618 import edges with
zero missing libraries, missing exports or forwarding cycles, using the actual
candidate Wine API-set schema. Presence does not establish correct behavior.
The tool
classifies `sceSystemServiceLoadExec` as a throwing stub because replacing an
executable is unsupported. Its specific `"exit"` branch is implemented and
exits the host with code zero, even when DOOM calls it after a fatal error.
A successful process exit alone is therefore not evidence of gameplay.
Four further guest-libc diagnostics for error reporting, backtraces and module
lookup remain throwing stubs. The TLS import is bound to the converter's
generated Windows resolver, as confirmed in the emitted startup code, rather
than calling the host TLS stub. These static checks do not establish which
error paths a live game will reach.

The original, separately licensed changes are kept as reproducible patches in
[`runtime-patches/anyps5/`](../runtime-patches/anyps5/):

- **Limited null-descriptor capability negotiation:** keeps the actual feature
  disabled, permits no-image shaders and fully populated image heaps, and
  refuses direct or padded null image elements before guest address preparation,
  GPU allocations, descriptor updates or draw. Deferred construction and cached
  resource proofs use the same policy. This is not full null-descriptor emulation.
- **Vulkan portability negotiation:** enables the advertised portability instance
  extension together with its matching enumeration flag, and the device subset
  extension only when advertised. No feature value is forced.
- **Verified guest arena ownership:** an API probe found that Wine's Windows query
  reports some occupied native Mac intervals as free. Both ordinary and placeholder
  reservations fail there with `ERROR_INVALID_ADDRESS`, while other intervals
  accept placeholders. The patch subdivides conflicting ranges within a finite
  budget and excludes conflicts and all unverified remainder. Only successful
  exact-address reservations are owned. Guest allocation, fixed placement, release,
  commit and shared mapping check this coverage; other reservation errors remain
  fatal. Adjacent exclusions are merged.

### Presentation and focused checks

The rebuilt independent Wine 11.0 candidate contains Vulkan Loader 1.4.350 and
unchanged official MoltenVK 1.4.2. An original Windows Win32 presentation probe
created the pinned engine's required-feature device while truthfully omitting
null descriptors, then created its surface and swapchain. All 4,096 uploaded
pixels matched after GPU readback. The independently compiled, corrected-count
probe presented 183 frames at 632 by 406 over a bounded three-second run. An
earlier equivalent probe presented 336 frames over five seconds.

Omitting the advertised portability extension and flag reproduced
`VK_ERROR_INCOMPATIBLE_DRIVER` (`-9`) at instance creation in both Wine and the
native comparison. That measured failure motivated the portability patch.
This verifies a device and transfer/presentation path; it does not validate
SDL integration, PS5 shader translation, game performance, input or audio.

The standalone capability/policy check passed 16 cases. The segmented-reservation
check passed 32 cases independently on ARM64 macOS and with the supported
Windows compiler under Wine. Full graphics tests initially failed before their
entry point because libc's static guest-arena reservation encountered the
occupied native intervals. With the reservation patch, the actual compiled
shader-profile and Graphics validation executables both entered their test
code and passed under Wine. A separate smoke using the rebuilt libc passed
owned allocation, private commit, write/read, placeholder reset, release and
reuse. It correctly refused ownership at the probed occupied intervals.
The shared-mapping regression passed 18 actual Windows operations under Wine:
private commit/reset across adjacent independent placeholders, two 16-KiB shared
views, a read/write alias, coalescing and release. Its first attempt used a
non-executable-capable section unlike production and was corrected in the fixture;
no production permission was relaxed. The focused fixed-mapping guard entry
passed all four refusal/preservation
cases, flexible allocation reuse and named mappings. The full upstream
GuestMemory suite then failed its existing `CheckMlock` working-set metadata
assertion: Wine 11.0 explicitly returns equal constant minimum and maximum
values from its working-set getter, and its setter does not change them.
The underlying host `mlock` implementation is real. No fake metadata or
full-suite success was substituted.

## Actual PS5 homebrew game results

The tested game was the converted public PS5 DOOM executable and its bundled
guest libc, with the shareware episode and the matched 116-PRX runtime plus
three exact GCC DLLs. A native Mac or Windows DOOM port was not substituted.
The game used its CPU renderer; AnyPS5 still presented its tiled framebuffer
through Vulkan compute/transfer operations and MoltenVK. The capture fallback
that decodes the buffer on the CPU was not enabled.

| Test | Observed result |
| --- | --- |
| Initial bounded 260-frame run | DOOM initialized, scripted menu/start/movement reached gameplay, six actual GPU-readback captures of the 1920 by 1080 framebuffer, exit 0 without timeout |
| Fresh GUI relaunch | Live game window rendered; keyboard `W` changed the viewpoint; right mouse fire reduced ammunition from 50 to 48 |
| Native window close | Process exited 0 without timeout before the configured frame limit |
| Save through game menu | A 32,094-byte slot-zero save was written, description `E1M1`, `version 110` header, episode/map 1, final marker `0x1d` |
| Load in a fresh process | Host log recorded reading the save, GPU frame showed the saved position; subsequent backward movement showed the room again; exit 0 without timeout |

All four game sessions returned zero without timing out. Success is supported
by logs, save-file validation, actual GPU captures and observed live input, not
by the exit code alone. The displayed game rate was approximately 35 FPS,
which is this homebrew's pacing; it is not a retail-game performance benchmark.

The SDL/AudioOut path opened at 48 kHz, but audible output was not measured.
A physical controller, sustained play and other PS5 games were not tested.
No claim is made for general PS5 compatibility or games using null image slots.

The public homebrew supports bounded test plans in `app0/test.cfg`. The initial
reproduction used:

```text
game doom1.wad
present cpu
input 40:OPTIONS:2;60:CROSS:2;80:CROSS:2;100:CROSS:2;130:LY=-32000:70
frames 260
```

For GPU readback evidence the isolated child process used
`APS5_DUMP_FRAMES=7`, `APS5_DUMP_FRAMES_EVERY=40`, `APS5_DUMP_SCALE=2`,
and no `APS5_NO_GPU_DUMP`. The process working directory contained the converted
EXE, `libs/`, `app0/` and `download0/`. Source and download provenance, exact
runtime/dependency hashes and sanitized results are retained locally under
ignored experiment folders; no game files or runtime binaries are published.

## Work needed before integration

1. Validate audible sound, a physical controller and sustained play. Save/load,
   rendering, keyboard/mouse input, window close and relaunch have the measured
   homebrew results above.
2. Keep games using null image descriptors unsupported until a complete shader
   and resource fallback exists. Dummy images alone are insufficient.
3. Address or correctly scope Wine's working-set metadata limitation before
   claiming the full upstream GuestMemory suite passes.
4. Package a reviewed, reproducible experimental engine route before exposing
   it in Caskrun. The normal app and engine chooser were not changed.
5. Test additional legitimately available PS5 inputs separately. One homebrew
   does not establish retail PS5 compatibility.

## Isolation and retained evidence

All experiments use ignored project `.build/` folders, separate Caskrun data
homes, prefixes and runtime bindings. Test profile host-folder links are replaced
with empty private folders and the host-root drive is removed before test code
runs. The earlier two test bottles were stopped and removed after process and
open-file ownership checks. The new compiler and game sessions were also
stopped; process-environment and
open-file checks confirmed zero owners before their disposable prefixes were
removed. Raw game logs, captures and private session records were removed.
The matching source, compiler/cache, dependency package and sanitized results
remain in organized ignored folders for reproduction.

Reviewed public source, verified upstream downloads, reproducible patches and
sanitized evidence are retained in organized experiment folders. Game files,
binaries, raw logs, captures, account details and device identifiers are not
committed. The installed app, user collection, existing bottles and active
Windows runtime are not modified by this investigation.
