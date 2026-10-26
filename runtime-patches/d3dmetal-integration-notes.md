# Optional D3DMetal host boundary

These original Wine 11 source adaptations are development experiments. They
are not enabled in the packaged runtime and do not establish D3DMetal support.
The Apple evaluation component must be obtained separately under its license.

## Public contracts

The non-native-region patch adapts the two interfaces published by Apple:
`supports_non_native_code_regions(void)` and
`register_non_native_code_region(void *start, void *end)`.

- [Apple's published formula and source patches](https://github.com/apple/homebrew-apple/blob/2bc44284e24d39ed64d6f492a0e1f4c47a5ced08/Formula/game-porting-toolkit.rb)
- SHA-256: `7a124b8e74edd3f453ef366e4e103608857801fbc5e085dc6fe885d57b6c9568`
- License: LGPL-2.1-or-later.

The handle stays open for the process lifetime. No unregister contract is
published. Only non-builtin application PE image mappings are registered,
including the main executable. Existing Wine GS, TSD, syscall and signal
behavior is unchanged. The absolute library path is an explicit opt-in via
`CASKRUN_D3DMETAL_SHARED_LIBRARY`.

The native-import patch implements original typed adapters for nine NT APIs
whose Microsoft x64 calling convention is described in
[published host declarations](https://github.com/Alien4042x/WineForge/blob/df3d347fd59ebe932affeb83c6a39fe34abf39e8/dlls/ntdll/unix/unix_private.h).
Only interface facts were used; the implementation is not imported.
The reference file's SHA-256 is
`9b811058545eb74fc933e994e8b4408cfef5fe11e95e8dcc1f42081b70c87036`.

The original Wine SysV bodies stay intact. Compiler-generated MSABI wrappers
forward into them. A source-built tail selector uses only volatile R10/R11,
preserving argument registers and the stack. Only a caller in the selected
support dylib's executable `__TEXT` range receives the MSABI route. Bounds
come from public `dladdr`/`getsegmentdata` metadata, are checked for overflow
and stay fixed for the process lifetime. With no opt-in, all calls use SysV.

The separate native-thread patch handles a selected support-library thread
start through an original PE relay and an appended NTDLL Unix call. Wine's
normal PE/native transition supplies the thread's native state before the
typed callback runs. Wine keeps the original thread entry and argument for
the server and retains its normal thread initialization and exit behavior.
The relay applies only to x64 starts inside the validated selected library;
ordinary Windows starts and WoW64 keep their existing route. It allocates no
extra context for a suspended thread. The NTDLL PE and native halves must be
built and staged together.

The macdrv patch implements the 24-slot `macdrv_functions` contract declared
in the same pinned Apple formula. Its ten native-ABI slots use actual Wine
windows, views and Metal devices; its fourteen Microsoft-ABI slots dispatch
synchronously to real Wine PE registry, monitor and window APIs. The historical
120-byte window structure is a separately allocated snapshot, never a cast of
Wine 11's different private layout. The source patch is an original
LGPL-2.1-or-later implementation; no Apple or CrossOver implementation is
imported.

The snapshot retains the actual Cocoa window and matching current client view
under Wine's window-data lock before detach can dispose of them. Unknown or
detached parents are refused. Snapshot and Metal-view ownership are tracked
separately; view release waits for in-flight layer calls and permits a new view
on a still-owned snapshot. Borrowed layers remain valid only while their view
is owned. No adapter map lock spans synchronous Cocoa work or a PE callback.
Existing DXMT exports are preserved.

Monitor enumeration uses a typed PE callback relay and an appended, owned
winemac Unix-call entry to invoke the native Microsoft-ABI callback through
Wine's ordinary PE/native transition. The original callback, `lparam`, early
stop result and LastError are preserved. Existing Unix-call indices remain
unchanged. The PE and native driver must be built and staged together because
their private initialization transport and callback table are paired. The
WoW64 transport table is updated too; runtime validation remains x64 only.

## Reproduction limits and preparation order

The Wine-hosted probes currently require a manually prepared private source
tree, configured Wine build and candidate runtime. They verify and exercise
those inputs; they do not reconstruct the full candidate from a source
checkout. The candidate receipt is an integrity-gate input, not a build or
installation command. There is no complete one-command D3DMetal preparation
workflow yet. The small native argument-contract probe builds its own fixture,
but that does not remove the Wine-hosted tests' preparation requirements.

The upstream source is the official
[Wine 11.0 archive](https://dl.winehq.org/wine/source/11.0/wine-11.0.tar.xz),
pinned by `WINE_SHA512` in `scripts/build-wine-native-text-dev.py`:
`a2c3db14f8cf0d19927466805c8f17c68ee7e93d1196d1162dd2279af497c21ec611a63f7a9de59953bbdfdb44c87d7bf55373c6533224a5d54e434c29428d1b`.

Manual preparation must preserve these source and pairing dependencies:

1. Reconstruct the reviewed Caskrun Wine 11 baseline and its corresponding
   dependencies. The ordered `BASE_PATCHES` and `UI_PATCHES` lists are in
   `scripts/build-wine-native-text-dev.py`; the Steam reopen extension follows
   them in `scripts/build-wine-steam-reopen-dev.py`. The coordinated MSync and
   Vulkan-portability additions are recorded by
   `scripts/build-wine-engine-components-dev.py`. Keep the matching reviewed
   common wineserver and client protocol; an arbitrary Wine installation is
   not an equivalent input.
2. Apply `wine-11.0-winemac-metal-exports.patch`, then
   `wine-11.0-dxmt-client-surface.patch`, before the macdrv host adapter.
3. For NTDLL, apply `d3dmetal-wine-11.0-non-native-regions.patch`, then
   `d3dmetal-wine-11.0-native-import-abi.patch`, then
   `d3dmetal-wine-11.0-native-thread-start.patch`. Apply
   `d3dmetal-wine-11.0-macdrv-host.patch` to the separately prepared driver
   source. Verify the resulting sources against the reviewed patch set.
4. Build matching NTDLL PE/native components and both driver PE architectures
   with their native driver. Stage them together only in a fresh, owned
   private candidate. Mixing an older PE half with the new native half is
   unsupported because their Unix-call tables and private transports differ.
5. Obtain Apple's official evaluation component separately through the
   [Game Porting Toolkit page](https://developer.apple.com/games/game-porting-toolkit/)
   under its own license. Validate that input and retain its original bytes
   and signature. The Apple component, candidate runtime and local proof
   records are not supplied by this source repository.
6. Record the exact source, paired-image and original-probe identities for the
   candidate, then run the independent adapter gates before the combined
   Apple test. Tests use fresh owned prefixes; installed apps and user bottles
   are not preparation inputs.

This is dependency/order guidance, not a verified stand-alone command recipe.
Toolchain configuration, dependency preparation, candidate staging and receipt
creation still need manual work. The failed combined result below remains the
acceptance status even when all preparation and adapter checks succeed.
An independent reconstruction of the NTDLL source tree from the pinned Wine
archive and 13 ordered patches matched all 12,430 entries, including file
contents, modes and symbolic links. This source check does not itself build or
validate the complete runtime or Apple integration.

The exact audited **NTDLL** patch order is:

```text
wine-11.0-steam-cef-commandline.patch
wine-11.0-steam-dock.patch
wine-11.0-native-app-identity.patch
wine-11.0-native-text-editing.patch
wine-11.0-steam-native-window.patch
wine-11.0-steam-native-bundle.patch
wine-11.0-steam-quit.patch
wine-11.0-steam-reopen.patch
wine-11.0-msync.patch
wine-11.0-vulkan-portability.patch
d3dmetal-wine-11.0-non-native-regions.patch
d3dmetal-wine-11.0-native-import-abi.patch
d3dmetal-wine-11.0-native-thread-start.patch
```

The separate driver's Metal-export, client-surface and macdrv-host adaptations
are not part of that 13-patch NTDLL source tree. Its complete reconstructed and
candidate inventories both matched
`c332fefcd13a08130dd280a240ec95582fe2500896e1dbe3a3b374a767b904c0`,
with no excluded entries. The final private candidate receipt binds the audit
record and inventory hashes alongside the actual paired-image identities.

## Validation

`scripts/d3dmetal-native-abi-probe.py` extracts the exact thunk macro and typed
wrapper bodies from the patch. Its independent native fixture verifies all
nine signatures, including eleven arguments for `NtCreateThreadEx`, through
both conventions: 27 calls, no argument mismatches, and eight selector boundary
cases. This tests argument transport and return values, not NT semantics.

`scripts/d3dmetal-native-nt-semantics-probe.py` uses a fresh owned prefix and a
clearly named mock support dylib to exercise real Wine NT semantics from both
conventions. Event state and previous values, semaphore counts, handle closure,
and an owned suspended thread's creation, termination and wait all passed for
both callers. The fixture reached its PE main and exited zero. The common
server was stopped only for its owned prefix and independent checks confirmed
no remaining owners. This fixture never loads the Apple framework.
Both supervisor modes are nonmutating: candidate images must be staged
separately. The default accepts only the exact original PE/native NTDLL pair;
`--existing-reviewed-candidate` validates the four-patch source proof and exact
current NTDLL/driver/common-executable hashes without copying or signing an
image. Live owners are refused. A failed assertion or incomplete cleanup
returns nonzero; successful cleanup requires verified removal of its owned
prefix and unchanged candidate image hashes.

`scripts/d3dmetal-native-thread-probe.py` separately exercises a real native
thread callback using an original mock support library. Before the thread
relay, an ordinary Windows thread passed. The native callback entered,
completed `getpid` and its initial `pthread_getspecific`, then did not return
from its own `pthread_setspecific`; the process exited with code 5. Fixed
progress markers were ordinary output written solely by the fixture, not
private-process memory or TLS inspection. With the relay, both a selected
Microsoft-ABI caller and an unrelated SysV caller created, resumed and joined
the same selected native callback. Public pthread key set/get and per-thread
separation, owned event signaling and exact thread exit status passed, as did
the ordinary Windows thread control. All four generated C contracts were
identical to the failing baseline. The nine-NT-API fixture was then rerun in
nonmutating existing-candidate mode against the unchanged reviewed paired
candidate and passed again under both calling conventions.
Both owned prefixes were removed after no-owner checks. These are mock-host
results; they do not establish that the thread issue caused Apple's failure.

`scripts/d3dmetal-macdrv-host-probe.py` passed 38 sequential assertions through
the actual source-built x64 driver and its real PE callback dispatcher. The
fixture verified all 24 table slots and 14 callback APIs, registry values,
LastError preservation and failure propagation, monitor enumeration and nested
monitor information, early stop, empty clipping and forced display refresh.
It also covered real Metal-device/view/layer creation, same-snapshot view
recreation, unknown and double release, and refusal after the owned HWND was
destroyed. The exact paired PE/native driver images were verified in that
same live process. No Apple component was loaded. The fixture completed its
PE main and removed its owned prefix after independent ownership checks.

The initial direct PE-to-native monitor callback failed, while an independent
count-only PE relay completed enumeration. The final ordinary Unix-call
transition allowed the real native callback, including its nested monitor API
call, to complete. No GS/TSD manipulation or Apple callback-table patch was
added. These are sequential API results, not a concurrency stress test, an
Apple graphics result or a game-compatibility claim.

Both x86 and x64 driver PE files compiled. The separate WoW64 window control
did not satisfy its exact mapped-driver gate and is not an accepted runtime
pass. The successful default fixture explicitly records that WoW64 lifecycle
was not tested; x86 application/UI compatibility is not established by this
adapter experiment.

Separately, the restricted native-host control established factory, device and
256-pixel clear/readback with the official framework outside Wine. That control
does not establish application, swapchain or game compatibility.

The verified adapters still do not establish a working Wine-hosted Apple
factory. Unknown private initializer signatures must not be guessed. Apple
framework state, code and imports remain unchanged.

## Combined Windows API gate

`scripts/test-d3dmetal-host-dev.py` runs the original
`scripts/probes/d3dmetal-smoke.c` fixture only against an explicitly prepared
private candidate. The fixture first creates its own hidden HWND and HDC,
initializing Wine's normal USER32/macOS driver before loading DXGI. It then
uses the existing hardware-only D3D12 clear/readback test with its mandatory
live mapped-image gate. No runtime image is built, staged or modified by this
supervisor, and no store, game or user bottle is involved.

The candidate receipt binds the reviewed adapter patch hashes, paired NTDLL
and window-driver hashes, the original probe executable and both probe C
sources. The supervisor verifies the separately licensed, unmodified Apple
input and framework signature, the selected images in the same live owned
Windows process, region support and main-PE registration. It verifies image
integrity again afterward and stops/removes only the fresh owned prefix.
Bounded API/mapping evidence remains private. Eleven parser and source-proof tests
reject missing/duplicate API checks, failed assertions and nonzero exits;
those parser tests do not execute Wine or Apple code.

One combined test used the three initial reviewed adapters, including macdrv
patch SHA-256
`347ce55b13449293b7c836410b614c65ebe08367f3ceafe3840b62f7befb5d8e`.
After the independent native-thread defect was fixed and its unchanged
functional fixture and nine-NT regression passed, a further combined test used
all four reviewed adaptations. The additional native-thread patch was
`c2b7b40f5f776aabd07efed913283cf662dfd39a858b5bb49c5e29d9a789b6ce`.
The final candidate was bound to the complete 12,430-entry source audit above.

The strict MinGW probe build passed. In both combined attempts, the actual
owned hidden HWND/HDC, USER32/macOS driver initialization, logical DLL hashes,
same-process mapped-image gate and factory export lookup passed. The gate
included the exact paired NTDLL/window driver and the unmodified Apple PE,
support library and framework. Non-native-region support was enabled and the
main application PE was registered.

Both processes exited with code 5 before `CreateDXGIFactory1` returned.
No successful factory HRESULT, Wine-hosted device, render/readback, swapchain
or game result was established. The adapters are verified interface work but
are insufficient to resolve this factory failure; its cause remains
undetermined. The native-thread fix does not establish that its separate TLS
defect caused this Apple failure. In both attempts, Apple's original signature
and all checked Apple/paired-host fingerprints passed post-probe integrity;
the fresh owned prefixes were removed after independent ownership checks.
Installed apps and user bottles were unchanged. These x64 API experiments do
not establish x86 application or graphics compatibility.

D3DMetal therefore remains unavailable in the packaged app. Enabling it still
requires an actual combined factory/device/render/lifecycle pass. A local
evaluation license acceptance does not authorize redistribution of Apple's
component; its binaries, local receipts and raw diagnostics remain private.

## Separate mock GFXT transport

The original [Wine GFXT mock probe](../scripts/probes/gfxt-wine-mock/README.md)
uses its own PE/Unix protocol and partial, attributed MIT-licensed UTM interface
declarations. In an owned Wine prefix, seven checks passed, including reading a
real Wine registry value, a negative-value case and releasing its own mock
factory. The same process mapped the expected Wine and fixture images without
loading Apple's framework or support library; no native object pointer crossed
into PE code. The test runtime and prefix were removed after ownership checks.

This verifies only a small transport and registry boundary. It does not
implement the other host services, Windows graphics exports or COM proxies,
and it does not establish D3DMetal, presentation or game compatibility.
