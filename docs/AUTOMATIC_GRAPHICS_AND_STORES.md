# Automatic graphics and publisher stores

## Source and scope

Reviewed [MacadeKit](https://github.com/abduzeedo/mapc-engine/tree/37bb046423dbb85c5cb98fee742868dccb96002d)
at revision `37bb046423dbb85c5cb98fee742868dccb96002d`. Its graphics inspection
and EA/Ubisoft handling informed the Rust adaptations. The upstream Apache 2.0
license and notice are retained under `third-party/mapc-engine/` and copied into
local app builds. Caskrun keeps its existing Collection folders, inspector,
Discover page, artwork policy and navigation.

No MaPC binary, proprietary interface, CrossOver-derived engine or Apple toolkit
component was imported. This change uses Caskrun's existing reviewed Wine/DXMT
package and does not add an engine download or account connection.

## Automatic graphics

At a verified idle launch, Caskrun inspects store-declared installed game folders
and registered EXEs. The read-only scanner parses PE architecture plus ordinary
and delayed DLL imports, and follows bounded same-directory engine DLL imports
(for example UnityPlayer). It never executes the inspected files. Known store
clients, crash handlers and Steam redistributables are excluded as game evidence.

- Positive x64 Direct3D 10/11 evidence can select the verified DXMT view.
- D3D12, D3D8/9, Vulkan, OpenGL, mixed architectures, unrecognized requirements,
  malformed files or an incomplete scan keep WineD3D.
- A manual WineD3D or DXMT choice remains unchanged. Existing saved DXMT options
  and display preferences are preserved.
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

## Validation

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
