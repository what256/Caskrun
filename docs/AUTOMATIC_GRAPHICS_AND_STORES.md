# Automatic graphics and publisher stores

_Current source behavior updated 26 October 2026. The build-55/V14 validation
below is historical; the new hybrid engine's API evidence has its own scope._

## Source and scope

Reviewed [MacadeKit](https://github.com/abduzeedo/mapc-engine/tree/37bb046423dbb85c5cb98fee742868dccb96002d)
at revision `37bb046423dbb85c5cb98fee742868dccb96002d`. Its graphics inspection
and EA/Ubisoft handling informed the Rust adaptations. The upstream Apache 2.0
license and notice are retained under `third-party/mapc-engine/` and copied into
local app builds. Caskrun keeps its existing Collection folders, inspector,
Discover page, artwork policy and navigation.

The original MaPC adaptation imported no MaPC binary, proprietary interface,
CrossOver-derived engine or Apple toolkit component. That phase used the
reviewed Wine/DXMT V14 package without an engine download or account connection.
The later independent hybrid DXVK/MSync candidate and separate private Apple
evaluation are described in [backend status](GRAPHICS_BACKEND_STATUS.md).

## Automatic graphics

At a verified idle launch, Caskrun inspects store-declared installed game folders
and registered EXEs. The read-only scanner parses PE architecture plus ordinary
and delayed DLL imports, and follows bounded same-directory engine DLL imports
(for example UnityPlayer). It never executes the inspected files. Known store
clients, crash handlers and Steam redistributables are excluded as game evidence.

- Positive x64 Direct3D 10/11 evidence can select the verified DXMT view.
- Complete unmixed Direct3D 9 evidence can select the verified hybrid DXVK view
  for x86/x64 Windows apps. It requires the separately validated D3D9 component;
  a modern DXVK version string alone does not authorize that decision.
- D3D12, D3D8, Vulkan, OpenGL, mixed APIs, unsupported/unknown architectures,
  unrecognized requirements, malformed files or an incomplete scan keep
  WineD3D. DXMT requires x64 evidence without x86 callers; the D3D9 hybrid route
  can support either or both validated Windows architectures.
- A manual WineD3D, DXMT or DXVK choice remains unchanged. Existing saved DXMT
  tuning, MSync and display preferences are preserved.
- Automatic is a bottle decision: every app, child, native alias, Steam reopen
  request and shutdown proof consumes the same recorded view. It never changes
  underneath a running client or game.
- Newly installed games are considered after the bottle's apps quit and it is
  opened again. An uninitialized bottle starts with the existing WineD3D
  bootstrap; automatic inspection takes effect on a later idle launch.
- The local `auto-graphics.json` decision contains only version, renderer and a
  fixed reason code. Duplicate/export omit it, and import discards it, so a
  copied bottle re-evaluates on its own host. The portable manual preference
  remains in `graphics.json`.

The scanner has per-target limits (depth, entries, images and bytes) and a
32-target bottle limit. No-follow descriptor reads refuse symlinks and changed
files. Cancellation interrupts traversal and PE reads. Static imports cannot
identify every API loaded dynamically or every engine DLL outside the local
import graph; detection is a configuration hint, not a compatibility or speed
measurement. DirectX 12 support is not introduced by this change.

Unavailable required components keep Automatic on WineD3D with a fixed
explanation. A saved manual choice is not silently changed. Runtime capability
comes from the exact reviewed package and pinned component receipt, not from
an arbitrary DLL, a mutable host driver path or upstream compatibility claims.
Normal launches and pure control requests apply the same selected graphics
environment; those control requests do not create caches or repair prefixes.

## Other stores

EA setup now checks numeric version directories for `EADesktop.exe` when Wine
has not created the usual current-version junction. A plain saved EA client
whose reviewed old path disappeared can be reconnected to the checked current
client in the same bottle. Its saved shortcut identity and title are retained.
Existing files, links, custom arguments and working directories are preserved.
Only checked EA clients receive the fixed software-CEF browser switches; game
renderers and generic apps receive no such switches.

Installed EA games are discovered from bounded `DiPManifest/contentIDs/contentID`
metadata in the normal EA Games directories. Ubisoft game IDs and locations
come from bounded machine installation records. Neither flow reads account or
authentication files. Publisher game discovery still includes Epic and GOG.

EA/Ubisoft Play requests revalidate the installed game and same-bottle saved
client before sending only the fixed `link2ea://launchgame/...` or
`uplay://launch/.../0` operation. The publisher handles sign-in, ownership,
updates and actual game launch. A store window need not remain visible while a
game runs, so these URI requests do not wait for that window or claim game
readiness. Epic/GOG retain their existing Open store behavior, with current
manifest and client checks.

## Current engine API evidence

The 26 October coherent hybrid passed original D3D9 and D3D11 GPU probes in
both Windows architectures with exact selected-module and host-driver mappings,
readback, presentation and lifecycle checks. The matching MSync client/server
port passed its off/on WineD3D/DXMT synchronization matrix. These engine tests
do not prove an Automatic choice for every game or additional EA/Ubisoft client
support. No storefront or game was launched in this engine pass. See
[backend status](GRAPHICS_BACKEND_STATUS.md) for exact pins, counts and limits.

## Historical MaPC adaptation validation — build 55/V14

The full Rust suite passed 635 unit and 7 integration tests, with 3 existing
tests ignored. The final scanner run passed 25 checks, including an additional
EA helper-folder regression added after the full run. Python publication,
runtime-package and DXMT-option checks passed 35 tests; Swift preference
contracts passed 24 checks. The full Swift app compiled, formatting passed,
and Clippy reported no warnings in changed files (older warnings remain).

The original x64 `scripts/probes/auto-graphics-smoke.c` statically imports
`D3D11CreateDevice`. In a fresh, isolated bottle, the packaged build began on
Automatic/WineD3D, discovered that registered executable, and selected
Automatic/DXMT with reason `d3d11-detected`. A hardware feature-level 11.0 device
cleared a texture and read back RGBA `[32, 64, 128, 255]`. WineMetal was loaded;
WineD3D was not. Ownership and idle checks passed, and the disposable bottle
was removed. Reproduce with `scripts/test-auto-graphics-runtime.py`; it uses
the existing fixture harness and never selects real user bottles.

Local build 55 passed deep signature checks and disk-image verification. It
uses the unchanged reviewed V14 runtime. MaPC notices and the new dependency's
license are included. The staged source passed the publication privacy gate.

These fixtures do not establish EA/Ubisoft authentication, game downloads,
gameplay or performance improvements. No real publisher account was used in
this phase; the earlier launcher validation results still apply until those
real-client journeys are tested.
