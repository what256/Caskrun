# UI smoke — 7 October 2026

This UI pass simplifies Caskrun while preserving its existing operations. It does not add graphics engines or establish Windows application compatibility.

## Current layout

- A floating capsule contains Home, Discover, and a plus menu with Discover apps, Install from file, New bottle, Import Caskrun backup, Import from CrossOver, and Manage bottles. Home and Discover have no large title/search/action block, dedicated Bottles page, global Settings screen, or interior branding.
- Home starts with one small folder tile per active bottle, including empty bottles and those whose apps are hidden. One click opens its apps and Steam games beside the right inspector. A compact All bottles breadcrumb returns to the folder grid. Steam retains its white icon backing at the smaller app-icon size.
- Single-clicking an app or game selects it; native double-click, the inspector Open button, or Command-O launches it. Steam games launch through saved Steam in their own bottle. The native click router requires a first click on the actual app/game; the second click after opening a folder cannot launch a newly revealed tile.
- The inspector retains app arguments/visibility, installation/discovery, Display & Graphics, Windows Tools, Files & Maintenance, Manage Bottle, and Recently Deleted/Restore. Its bottle picker also changes the displayed folder. Game selection scrolls back to its Open control. Closing the inspector via its x button or Escape returns to the all-bottle grid.
- Command-1/2 navigate Home/Discover; Command-3 opens bottle management; Command-N opens the installer picker. Command-F reveals a compact search field in the capsule; closing it clears the query. Navigation and folder changes clear old filters.
- Selection and folder/panel changes are immediate, with stationary color/underline hover feedback. Steam manifest discovery refreshes when a folder opens, when Caskrun becomes active, and every 15 seconds while that folder is visible. Existing cached game results remain visible during reads.

## Earlier simplification build observations

- Home displayed Steam with its requested icon backing, close name, and background-free three dots.
- Command-F focused Home search. An unmatched query showed no results; Clear search restored Steam.
- Discover showed all eight publisher logos and cards. Command-F focused its search; `epic` filtered to Epic Games Store.
- Epic's setup card opened its setup sheet. Cancel returned to Discover without starting an installer.
- Bottles displayed compact apps and collapsed control sections. Expanding Windows Tools and Files & Maintenance revealed their actions. The screenshots showed Wine Configuration, Game Controllers, Task Manager, Internet Settings, Registry Editor, Command Prompt, Simulate Reboot, Run an EXE, C: Drive, Uninstall, Quit Windows Apps, and Logs.
- The bottle menu retained Rename, Duplicate, Export backup, and Move to Recently Deleted. The Home menu retained Bottle Controls, Advanced argument actions, C: Drive, Logs, Quit, and Hide.
- Command-F from Bottles opened Home and focused its search.

The menu entries were inspected without running their tools or changing the Steam bottle. The Home nested graphics action was present but the UI driver could not activate its submenu reliably; its expansion and scroll path were reviewed in source. Empty/hidden libraries, multiple bottles, large app libraries, keyboard-only navigation, and recovery flows were not exercised in this smoke.

The final candidate's screenshots also confirmed the complete two-line Rockstar Games Launcher name, the single-bottle layout without a duplicate selector, and all three collapsed control sections in one window. Expanding Display & Graphics showed High Resolution Mode off at 96 DPI, WineD3D as the default, and DXVK/DXMT/D3DMetal/MSync as not available yet. No display setting was changed.

## Package

The ARM64 build, catalog validation, architecture and signature checks, disk-image verification, and published checksum passed. The preceding UI-refinement candidate replaced `dist/Caskrun-0.1.0-arm64-dev.dmg`; SHA-256: `0ae8a56a0be594492eabcf4568b16c85624be8f2aae9bd602c486da3ce67df2f`. The final app was opened as the only Caskrun UI process.

## Floating navigation follow-up

The later candidate replaced the full-width header with a small centered capsule. The Home screenshot confirmed the rounded border, selected Home pill, and compact plus button, with the page content unobstructed. Command-2 opened Discover; the plus button opened the Windows installer picker, and Cancel returned without selecting or running an installer. Command-1 restored Home. The native window and menu-bar behavior were retained.

The floating-navigation package passed ARM64 build, catalog, signature, architecture, disk-image, and published checksum checks. That candidate’s DMG SHA-256: `2d614941800f8cca070a99d3c054197743b686836e35cd9915428e5001f23321`.

## Home inspector follow-up

The final candidate opened Home with only Home/Discover in the floating bar and no three-dot target on Steam. A single click selected Steam and opened the right inspector; a process check confirmed that this selection had not started Steam. Screenshots showed the rounded panel, close button, app Open action, and all five control groups. Expanding Display & Graphics showed 96 DPI, High Resolution Mode off, and WineD3D; no display setting was changed. Windows Tools exposed Wine Configuration, Game Controllers, Task Manager, Internet Settings, Registry Editor, Command Prompt, and Simulate Reboot. Files & Maintenance and Manage Bottle exposed the existing file/log/quit and rename/duplicate/export/recoverable-delete actions.

In the final published build, a native double-click on the plain Steam tile started one Steam main process and its web helpers. Caskrun hid the main window while keeping its process alive; reopening the same app restored Home and the inspector. This validates gesture routing and the existing launch handoff, not Steam sign-in rendering, Dock focus, account login, or games. The Saved Arguments editor opened from App Options and Cancel returned to Home without changing arguments.

The source review checked empty/hidden bottle access, current-model selection after refresh, recoverable deletion, moved menu routes, busy/runtime launch guards, and the retained action inventory. Those management and recovery paths were not exercised on the user's Steam bottle. Multiple app selection, simultaneous operations, minimum-window layouts, and keyboard-only navigation still need broader UI testing.

That inspector candidate passed ARM64 compilation, catalog validation, architecture, ad-hoc signature, disk-image, and published checksum checks. Its DMG SHA-256: `0bea4fddccf3dc3f31635c93745aa2577774a3515f3003bfcec08860c6d03801`.

## Immediate response and stable motion follow-up

Removed the deliberate single-click wait and the Home container's animation of grid reflow. Home icons no longer grow on hover; only their name color and small underline change. Panel open/close now updates immediately with no slide, resize animation, or spring. This also avoids spatial motion when Reduce Motion is enabled. The close button has a stationary hover target and a native Escape shortcut.

The local click monitor retains the first real mouse-up's app ID, window, point, and timestamp. It consumes only a matching second native mouse-down/up, then resolves and launches the original ID from the current model. It clears selection capture on other clicks, scrolling, keys, explicit inspector actions, or leaving Home; it does not delay keyboard/accessibility selection. A temporary focused AppKit/model fixture passed 87/87 assertions, covering target/time/point/window boundaries, released windows, cancellations, consumed events, one callback, fresh original-ID selection, and launch guards. Fixture receipts and rerun commands remain in `.build/home-click-router-checks/README.md`.

The packaged UI smoke showed the right panel after one click. Closing it and double-clicking Steam started one Steam main process while Caskrun hid its main window and retained its process. The final package's single-click and native Escape-close actions both passed. Display refresh preserved the observed 96-DPI/off state before and after a read; no setting or saved arguments were changed. Cached display state now remains visible during reads, with a small reserved loading indicator; failed reads invalidate stale state. Switching the selected app or bottle scrolls the inspector back to its header in source. Multiple installed apps/bottles, scroll-reset behavior across them, and frame-by-frame response timing were not exercised in the live UI.

The finished update passed ARM64 compilation, catalog validation, architecture, signature, disk-image, published checksum, and whitespace checks. That response candidate’s package SHA-256 was `f0e56619ae8fd6296d30c9c599cc2d668daccffd32c857015575025184a2752b`.

## Bottle folders, plus import, and automatic Steam games

The 7 October folder candidate passed Swift typechecking and ARM64 app/core build, catalog, signature, architecture, and DMG/checksum verification. The final Rust suite passed **169 tests, 0 failures, 2 optional tests ignored** with one test thread. An initial fully parallel run hit an existing two-second wait-launch timeout; that test subsequently passed alone, with its related group, and in the final full serial run. No unrelated test deadline was changed.

New disposable tests cover CrossOver folder copy, registry/runtime exclusions, active-source refusal, source/staging races, Steam manifest identity and installation state, bounded parsing, bottle-local library paths, exact cached icons, and matching saved-Steam launch routing. The packaged CLI fixture also showed one installed game only in its own bottle, rejected a game launch without saved Steam before starting Wine, and refused a fake CrossOver import while actual Windows processes were active without changing source data or adding a bottle. Receipt: `.build/folder-cli-4faefbia/receipt.json`.

CrossOver import currently accepts a **win64 bottle folder** containing `drive_c` and `system.reg`. It copies installed app/data into a new bottle and excludes source engine files, the Windows directory, links/external drives, and CrossOver metadata. `.cxarchive` files are unsupported. Caskrun initializes its own runtime on first launch; some apps may need dependency repair or reinstallation. No real CrossOver bottle or installed game compatibility was tested.

The Mac was locked when native UI verification was attempted; unlocking is pending. Live folder navigation, the plus-menu picker, and the updated gesture routing still require the on-screen smoke check. The preceding Caskrun UI and its Steam process were left running. The folder candidate is `.build/bottle-folders-oct07/Caskrun.app`; published image is `dist/Caskrun-0.1.0-arm64-dev.dmg`.

Folder-candidate DMG SHA-256: `f876266dcbf9dd95ba72e56882fb0805a45dda34828883cbe953d71f30bf1f99`.

## Unlocked launch follow-up

The Mac was subsequently unlocked and the user asked to open the candidate. The older Caskrun UI was quit through its native Quit command, and `.build/bottle-folders-oct07/Caskrun.app` opened as the only Caskrun UI process. The existing Steam main process retained the same PID, showing that switching Caskrun UI builds did not stop it. Native Home showed the Steam bottle folder with no large heading block. One click opened its smaller Steam app tile, All bottles breadcrumb, and right inspector with Open, App Options, installation, and all five bottle control groups. No Windows app was launched or stopped during this folder check. Discover, the import picker, game selection/launch, and folder double-click suppression still need live checks.

## Quit, folder exit, and Discover refinement

The user requested that full Quit Caskrun also close all Windows apps it manages. The new AppKit termination callback defers exit while Caskrun cancels its own setup invocations, waits for active operations, blocks new launch/mutation calls, and runs the new `quit-all-bottles` command. Its own bottles are enumerated freshly and stopped using verified, prefix-scoped Wine controls. Partial failure identifies affected bottles and cancels host termination, reopening Caskrun to show the error. No global Wine signals or arbitrary recorded engine paths are used. The stop operation uses the existing per-bottle force-quit primitive; save prompts and graceful application-specific shutdown are not implemented. The core invocation has a 60-second cancellation deadline; in-progress bounded prefix cleanup is allowed to finish.

Sidebar x/Escape now calls the same root-navigation helper as All bottles, clearing the open folder, game selection, click capture, and filter. Discover uses two-column compact rows grouped by Game stores and Publisher launchers, with original artwork and separate Installed / paused / failed state beside explicit Open / Install / Resume / Retry actions. The large headings remain removed, and search stays in the optional floating toolbar field.

The final serial Rust suite passed **181 tests, 0 failures, 2 optional tests ignored**, including 12 new shutdown fixtures. A Foundation-only harness extracted the actual delegate and command-gate definitions and passed **126/126 checks**. It exercised pending/no-early-reply, repeated quit, success, failure/reopen, retry, and read/shutdown allowlists versus queued mutations. It used fake native application types and did not run AppKit, Wine, or user UI; receipts are in `.build/quit-lifecycle-checks/`.


## Packaged native verification of quit and Discover

The candidate `.build/quit-discover-oct07/Caskrun.app` opened after the older UI quit, while its existing Steam session continued. Home showed one Steam bottle folder. One click opened the smaller Steam app icon and all inspector groups. Clicking the inspector x returned to the all-bottle grid, removing the Steam breadcrumb and controls; reopening the folder and pressing Escape did the same.

Command-2 opened the redesigned Discover page. Its screenshot showed all eight original launcher logos in two columns, four Game stores and four Publisher launchers, complete names including Rockstar Games Launcher, Steam's Installed/Open state, and Install actions for the other entries. Command-F revealed capsule search; `epic` left only Epic Games Store, and closing search restored all eight entries. The + menu exposed Discover, file installation, new bottle, Caskrun backup import, CrossOver import, and bottle management. No installer or import was run.

Immediately before testing full Quit, process reads showed one Caskrun UI, one Steam main process, and four Steam web helpers. Command-Q on the updated candidate completed without an error UI. The subsequent process check showed **zero Caskrun UIs, zero Steam main processes, and zero Steam web helpers**. Caskrun then reopened successfully and was left on Discover. This verifies full Quit against the existing real Steam session, not application save prompts, active game shutdown, simultaneous installer cancellation, or all Wine programs. Closing only the main window retains the existing menu-bar behavior; that was not retested in this pass.

The final ARM64 package passed compilation, catalog validation, architecture and ad-hoc signature checks, disk-image verification, publication, and checksum verification. The single visible development DMG is `dist/Caskrun-0.1.0-arm64-dev.dmg`; SHA-256: `19c0dc33702b41c678469dc03abfd0e802f04d80b451a62e032b065f7300e0d2`. Game compatibility and real CrossOver import remain unverified.


## Library and browser phase — 8 October 2026

The floating navigation now includes Library. Its compact filters cover All,
Steam, Epic, GOG, saved Apps and Browser, with a right detail panel. Discover
has Stores, Game reports and Browser games beneath its floating navigation.
Home keeps its bottle folders and adds saved browser shortcuts.

The complete SwiftUI source typechecks. Current automated coverage passes 420
Rust unit tests and 6 integration tests (2 existing unit tests ignored), 125
Python checks and 104 Swift checks. Timing-sensitive process tests passed with
one Rust test thread; no production or test deadlines were relaxed. Package
and command checks are recorded separately in launcher validation.

This phase has not yet had a live visual/keyboard pass: the Mac locked before
that check. Earlier UI results above concern their dated builds, not the new
Library. Actual game compatibility, browser gameplay and publisher sign-in
remain manual checks.
