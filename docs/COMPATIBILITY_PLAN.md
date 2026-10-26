# Compatibility implementation plan

_Status updated 26 October 2026. Caskrun is an open-source launcher in active development. Compatibility remains specific to each app and game._

The user-directed static reference review covered packaged setup orchestration, graphics configuration, shortcut handling and bottle tools. It identified architectural gaps; it did not execute CrossOver, provide its native GUI source, or establish Caskrun compatibility. No reference code, recipes, assets or runtime binaries were incorporated. The implementation work below uses original Caskrun code, publisher requirements, independently sourced upstream components and our own test results.

## Current completion pass

Automatic already selects DXMT for supported static import evidence; it is not
hardwired to WineD3D. Its frame pacing and spatial controls now remain available
when Automatic selects DXMT, and save/reset preserves the automatic decision.
EA startup feedback recognizes bounded numeric client-update folders. Local
packaging checks the app, Core, icon helper and runtime before replacement.
This pass uses source and isolated fixture checks; publisher apps and games are
left to the user's [final manual checks](NEXT_MANUAL_TESTS.md).

The separately licensed MaPC core informed the 19 October graphics/store
adaptation; its Apache notices remain in source and packages. See
[automatic graphics and stores](AUTOMATIC_GRAPHICS_AND_STORES.md). That work did
not import the proprietary CrossOver application or engine.

The [backend status](GRAPHICS_BACKEND_STATUS.md) distinguishes implemented
controls from missing engine work, including the newly documented Vulkan 1.4
route. DXVK, D3DMetal and MSync are still unavailable in the current runtime;
this pass does not turn an environment variable into engine support.

## Keep the ordinary flow simple

Discover → Install → publisher sign-in → install a game → Play remains the intended flow. Caskrun chooses its included engine and supported defaults. The Collection Windows details panel retains optional display, graphics, controller, diagnostic and maintenance controls. No global Settings page or Wine picker is needed for ordinary installation.

The public CrossOver guide describes choosing a bottle and dependencies for listed apps and publishing launch icons afterward. This is a useful workflow requirement for our own setup engine. Publisher agreements still require the user's decision. ([Mac user guide](https://support.codeweavers.com/en_US/crossover-mac-user-guide), [Steam setup](https://support.codeweavers.com/en_US/installing-steam-in-crossover))

## First: address observed failures

| Priority | Evidence | Next gate |
| --- | --- | --- |
| GOG | Its service fails to bind an additional IPv4 loopback address on macOS. An original service fixture passes basic Windows service lifecycle tests. | Test the original, narrowly scoped UDP compatibility prototype, including address reporting, cross-process traffic, endpoint cleanup and separate-prefix refusal. Then retry the real service and client in a disposable copy. Do not change the host network or infer a usable client from a successful bind. |
| Ubisoft / Battle.net | Setup completed and saved launchers, but client launches did not produce usable windows. Logs include nested exception or framebuffer messages without enough process attribution. | Identify executable, process, thread, loaded module and exception before selecting a component or graphics workaround. A framebuffer warning alone is not a proven GPU crash. |
| itch.io | Setup completed; the Electron client is blank. The stable bootstrap is a valid launch target. | Preserve its documented app-argument separator, identify the actual versioned child and compare a scoped diagnostic launch with normal launch. No blanket GPU or sandbox workaround. |
| Epic | The checked MSI failed with an execute-access fault. A separate original MSI fixture works. | Trace the real installer with a checked local copy and MSI logging; fixture success does not validate Epic. |
| EA / Rockstar | EA was closed at its agreement; Rockstar setup started but automatic approval blocked window inspection, so its owned test was canceled and cleaned. | Complete unauthenticated installer/client checks, with the user deciding any agreements. |

The [launcher validation record](LAUNCHER_VALIDATION.md) owns current outcomes and cleanup status. Installed, rendering, authentication and gameplay are separate stages.

## Original setup profiles

Caskrun currently checks and runs each publisher installer, retains a retry receipt and registers an expected executable. It lacks a reusable dependency graph and durable component-step state.

Implement versioned profiles with:

- Supported runtime capabilities and Windows architecture/version requirements.
- Reviewed publisher sources and independently verified component requirements.
- Ordered prerequisites with bounded checks, installer receipts, verification and restart recovery.
- Resumable per-step outcomes; cancellation must preserve completed steps and other bottles.
- Product detection and saved launch commands, followed by separate first-launch validation.

Do not combine historical profile variants, install arbitrary redistributables for every app, or declare success because an EXE exists. Determine each requirement from current publisher/upstream evidence and controlled tests. A future community profile update must retain the same review and provenance checks as bundled profiles.

## Graphics and synchronization

An explicit backend choice should be one mutually exclusive bottle setting. Automatic selection needs original, tested per-app/game profiles with a supported fallback. CrossOver documents a per-game database for Auto, while explicit DXMT, D3DMetal, DXVK or WineD3D choices apply bottle-wide. Its database is not Caskrun's data. ([Advanced settings](https://support.codeweavers.com/en_US/advanced-settings-in-crossover-mac-26))

| Capability | Current Caskrun state | Implementation gate |
| --- | --- | --- |
| WineD3D | Current included path; limited Direct3D probe success, no game validation. | Keep exact tested limitations visible in compatibility results. |
| High resolution | Real 96/192 DPI and Retina controls, idle checks, verification and rollback. **Validated on Steam:** with the control off its client is drawn at 1x and upscaled; with it on the same window is genuinely drawn at 2x. | Also measured on the engine's own builtin window, which is upscaled by default and matches native Retina applications with the control on. A DPI-unaware client such as the itch.io installer stays at 1x even with the control on, so it sharpens DPI-aware clients only. High Resolution stays optional and off by default; the user will test games later. |
| DXMT | Independently built, integrated experimental bottle control with exact package capabilities, preserved WineD3D default, idle changes and reviewed native-v2 migration/record rollback. The separate rendering experiment passes x64/x86 hardware feature levels 10_0/11_0, shaders/readback, HWND Present and resize/recreate/destruction checks. A user confirmed R.E.P.O. launched and worked with DXMT on 16 October after WineD3D device creation failed. | Test sustained play, visible output quality, performance, controllers and other titles/devices. See [current game results](LAUNCHER_VALIDATION.md), [app integration](DXMT_APP_INTEGRATION.md) and [DXMT smoke](DXMT_SMOKE.md). |
| DXVK / Vulkan | Older MoltenVK enumeration experiment; no successful Direct3D gate. The newer KosmicKrisp route has source-level Rosetta evidence, but its actual driver and Wine integration are not validated. | Independently package the driver/loader and matching Wine modules; prove x86/x64 device, draw, readback and window lifecycle before exposing a choice. See [backend status](GRAPHICS_BACKEND_STATUS.md). |
| D3DMetal | No integrated backend. | Separate component terms/source/distribution review and runtime validation. Never extract it from another app bundle. |
| MSync | Not implemented in current Wine 11. | Coordinated client/server implementation and synchronization correctness/performance tests; an environment toggle alone adds no support. |
| DXMT frame target | Optional bounded presentation-pacing control; original x64/x86 API matrix passed. | Measure displayed FPS, fullscreen behavior and real games; API-submission timing is not a performance benchmark. See [DXMT options](DXMT_OPTIONS.md). |
| MetalFX spatial scaling | Optional DXMT output scales with device capability checks; original x64/x86 matrix passed. Off by default. | Verify visible output, image quality and real-game performance. It does not change internal game resolution or provide temporal reconstruction. See [DXMT options](DXMT_OPTIONS.md). |
| DLSS / temporal scaling / frame generation | Not implemented. | Separate supported backend/vendor-extension integration and per-game tests; spatial scaling and frame targets do not supply these features. |
| Multimedia | Current Wine build disables GStreamer. | Independently source media components and validate codecs/playback. This gap does not by itself explain current launcher failures. |

DXMT's upstream runtime requirements include exported Wine `winemac` APIs; copying translator DLLs is insufficient. Its upstream repository publishes LGPL-2.1-or-later source and build instructions. Use pinned upstream source with notices and corresponding-source records, separately from the proprietary reference bundle. ([Runtime requirements](https://github.com/3Shain/dxmt/wiki/Device-System-Runtime-Specifications), [build guide](https://github.com/3Shain/dxmt/blob/main/docs/DEVELOPMENT.md), [license](https://github.com/3Shain/dxmt/blob/main/LICENSE))

The switchable DXMT development package carries a hashed component receipt with pinned source and patch records, both runtime views and an exact approved package identity. The core applies the bottle selection to setup, apps, children, tools and reboot; native identities and shutdown proofs follow the saved view. Selection preserves user DLLs and registry overrides, requires idleness and verifies its preference write. Runtime rollback restores the record before Wine starts; it does not roll back application data after Wine runs. Unavailable controls stay informational. See [app integration](DXMT_APP_INTEGRATION.md).

CodeWeavers' source page offers its FOSS components and recommends using component upstreams. It is not the full proprietary application or compatibility database. ([Official source page](https://www.codeweavers.com/crossover/source))

## Bottle and launch-command improvements

Wine Configuration, controllers, Task Manager, registry editor, Internet Settings, command prompt, session restart, quit, duplicate, backup and import already have Caskrun paths. Remaining acceptance and implementation work includes:

1. Use the same verified shutdown guarantees for explicit bottle quit and full Caskrun quit. A stopped Wine server must not hide remaining native client processes.
2. Preserve a complete shortcut command: executable, validated Windows argument parsing and checked working directory. **Arguments are implemented.** Discovery splits a shortcut's argument string with the quoting and backslash rules Windows itself uses, carries the result with the suggestion, and saves it as the launcher's arguments, so a suggestion starts the same program the shortcut does. Two shortcuts to one program with different arguments stay separate commands; a command the parser cannot reproduce exactly, such as one with an unterminated quote, is still skipped rather than offered in part. A checked prefix-relative C: working directory is now carried, validated on save/copy/restore and revalidated/pinned before launch. Unsupported requested directories suppress the suggestion instead of changing the command. Original x64/x86 Windows probes passed working-folder and relative-file reads.
   Portable default-folder handling is now implemented. Wine gives each bottle account absolute links to this macOS account's Documents, Downloads, Music and Pictures folders, to Videos as `Movies`, and to a Templates folder nested under `AppData/Roaming/Microsoft/Windows`, so copy and backup previously refused every initialized bottle. Caskrun now records each mapping as a folder name, never an absolute host path, and recreates it against the account that owns the restored bottle; an account without that folder gets a real folder inside the bottle instead. No link is followed and no host data is copied or created.
   One explicit standard-folder table governs both directions: a mapping is recorded on export and accepted on restore only when the exact path inside the bottle account and the home folder it names both appear in that table. Desktop is deliberately absent, because Caskrun replaces it with the bottle's own private Desktop before any copy. Any other path or target, including a home folder such as `Library` or `.ssh`, stops the copy instead of travelling into a backup or being recreated from one.
   Each backup records the Windows profile folder the exporting account used, and a restore reports whether this account can actually reach it. When that account has no profile folder in the restored bottle, one bottle-relative link is added beside the saved profile, so its `%USERPROFILE%` and `%APPDATA%` reach the profile the backup holds; a correct link the payload already carries counts as resolved without writing a second one, and the account that owns the recorded profile needs none. Nothing is ever renamed, moved, merged or removed: an existing profile folder, or a link this restore did not write, is kept and the restore returns a warning naming the recorded profile instead of claiming success. A manifest recording a profile the payload does not contain is refused, because the two then disagree about what was backed up. Caskrun only states that imported programs keep their settings and data when the profile actually resolved.
   Backups are written as `.caskrun-bottle` version 2. Version 1 backups predate the user-folder mappings and the recorded profile, carry neither, and still restore; a version 1 manifest that claims either field, and any other version, is refused.
3. Local raw-log export and privacy-limited structured diagnostics are implemented separately from ordinary launch options. Wine debug-channel controls remain pending.
4. Installed-program inventory, icon rebuilding, bottle notes, Windows-version controls and integrity-checked compressed backups are implemented; see [maintenance workflows](DEMO_MAINTENANCE.md). Component/dependency repair and runtime/profile update delivery remain pending.
5. Retain the current explicit warning that Caskrun's session restart closes apps. Verify upstream restart semantics before adding a non-disruptive alternative.

Each change needs appropriate source checks and an isolated live acceptance test. Keep test downloads, bottles and logs ignored, verify zero owners before cleanup, and transfer only non-personal results to project documentation.
