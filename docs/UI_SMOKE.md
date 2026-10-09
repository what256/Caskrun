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

After the Mac was unlocked, the packaged app opened as the only Caskrun UI.
Home retained its Steam bottle folder. Library showed the saved Steam app;
selecting it opened details, and Bottle controls returned to the Home folder
and inspector. No Windows app was launched by those selections.

Discover displayed all three scopes. Stores retained all eight launchers;
Game reports showed the offline catalogue, device summary, external source,
engine, renderer and hardware differences. The Steam destination sheet
selected the existing Steam bottle and cancelled without launching it.
Browser games opened a prefilled Hextris editor. Saving put that shortcut on
Home and in Library; removing the temporary shortcut returned the browser
count to zero. No browser gameplay was attempted.

Command-F focused Library search and filtering for Steam retained the saved
app. The top Add menu exposed browser links, Steam game installation, file
installation, a new bottle, Caskrun backup import and CrossOver import. The
import commands were inspected without importing user data.

Command-W hid the main window while the native process remained running.
Reopening the same app restored Library. The status menu itself was not
exposed by this automation surface, so its click-through remains a manual
check. Standard and maximized layouts were inspected; minimum-size layout,
VoiceOver and actual gameplay remain manual checks.

The visual pass found a localized Steam AppID (with grouping separators),
an Epic/GOG tooltip promising direct play, and a Library double-click that
could target a different tile after opening the inspector reflowed the grid.
The follow-up uses a plain ID string, source-specific action descriptions,
and the first click's identity for the second click. A named accessibility
action remains available for opening each item.

The older app's Quit exposed a separate engine mismatch before the update.
The existing matching-engine shutdown command completed, followed by empty
exact-bottle and old-runtime ownership checks; only the obsolete native UI
was then terminated for replacement. Bottle records were preserved. The
follow-up adds explicit reviewed Steam upgrade edges and a separately
full-attested predecessor-engine shutdown fallback; neither treats missing
or unknown engines as evidence that a bottle is idle.

The follow-up serial Rust suite passed **431 unit tests and 6 integration
tests**, with 2 existing unit tests ignored. Its 21 shutdown tests include
the reviewed-predecessor gates and refusal paths; a separate spawned-process
regression verifies shutdown environment isolation. The expanded Swift
browser/Library harness passed **89 checks**, including 42 click-routing and
action-description checks. Full SwiftUI typechecking and Rust formatting
passed. Clippy reported existing style suggestions; two new redundant
constant assertions were removed. No test deadlines were relaxed.

The optimized follow-up core completed `quit-all-bottles` against the existing
idle Steam bottle while the selected package differed from its reviewed
predecessor: `complete: true`, no failures and zero bottles requiring a stop.
The check took about 14 seconds, including full package hashes, and the
runtime-record bytes remained unchanged. This verifies the production digest
gate and idle server/owner path, not shutdown of a running legacy client.

The follow-up ARM64 app and DMG passed ad-hoc signature, architecture, disk
image and checksum verification. The packaged core repeated that shutdown
check successfully in about 15 seconds with unchanged runtime records.
Its SHA-256 is `9f57806a39e98fb3ff33d492c16c6435b371c5e37cf8c3d642adecca0a483b05`.
The runtime component receipt matches the previously tested v4 engine.
After unlocking, the final signed package replaced the standard local app
transactionally, after guarded bottle shutdown and empty ownership checks.
Library selection opened the saved app's details; Discover displayed all
eight stores, the report catalogue and all five browser entries. The Steam
destination sheet showed the plain AppID `1966900` and cancelled without
launching Steam. Close commands retained the native process, and selecting
the app returned its window. The automation target can reopen windows while
observing them, so direct status-menu click-through remains a manual check.

Native Command-Q displayed “Closing Windows apps…” and then exited the
observed process without an error. Reopening started one final Caskrun UI on
Home. No Windows client or game was launched, no graphics setting changed,
and the existing bottle records were retained. The temporary replacement
rollback bundle was removed only after the final app checks passed.

## Simplified Discover navigation — 9 October 2026

At the user's request, the floating navigation and status menu no longer
offer Library. Discover's second floating capsule is replaced by a compact,
left-aligned content filter row for Stores and Browser games. The Game
reports scope has also been removed. Home retains bottle folders, saved apps,
installed Steam games and saved browser links. Core manifest readers and the
reviewed evidence snapshot remain; the aggregate Epic/GOG inventory and
compatibility comparison are not exposed in the current UI.

The full ARM64 app compiled and its signed development package passed
signature, architecture, DMG and checksum checks. The packaged core SHA-256
and engine component receipt are unchanged from the preceding verified build.
The older UI completed native menu Quit before replacement. The new app
showed only Home and Discover, all eight store entries and five browser
entries; switching both content filters passed the on-screen check. No
Windows app, browser game, installation or bottle mutation was started.
The local DMG was updated, the known temporary rollback was removed after
zero-owner checks, and Caskrun was left open on Discover.

## Browser scene catalogue — 9 October 2026

The browser catalogue expands from five entries to 26 game and collection
links. Seventeen entries carry reviewed remote scene pictures; missing or
rejected previews use title cards. Discover keeps its compact Stores / Browser
games filter row and uses responsive picture cards. Known saved links reuse a
small scene preview on Home; arbitrary custom bookmarks keep their existing
fallback and trigger no image crawl. Own-file badges, collection actions and
unverified-link labels remain explicit. GTA 5 is omitted because the supplied
Wayback URL has no original destination.

Focused checks passed: **68 Swift cover-policy checks**, **89 existing browser
and interaction checks**, and **7 Python catalogue tests**. These cover exact
artwork authority, legacy metadata, unsafe URLs/redirects, session privacy,
byte/image bounds, downsampling and exact saved-link matching. The full ARM64
SwiftUI app compiled. These checks do not establish browser gameplay.

The candidate's app signature, ARM64 architecture, bundled catalogue and DMG
checksum were verified. The signed core hash and engine component manifest
match the preceding package; graphics defaults remain unchanged. Native visual
verification was initially blocked by the locked Mac.

After the Mac was unlocked, the older app completed its native Quit command
and the candidate replaced the standard local app after signature and idle
checks. One Caskrun UI process opened. Home retained the existing bottle folder;
Discover retained all eight stores and only its Stores / Browser games filters.
The expanded three-column grid showed scene previews for BO1, Kino, Cheese
Cube, Vibe Slops II, Surfd, Halo CE, Diablo and WASM Arcade. Picture-free entries
showed title cards. Search narrowed Halo, WASM Arcade, Hextris and BO1 correctly;
own-file badges, Browse games and unverified Visit website actions were visible.

A temporary BO1 bookmark changed its card to On Home and displayed the same
picture as a smaller Home preview. Removing that fixture through the native
confirmation returned Home to its original bottle-only state. No game or
installer was opened and no graphics setting was changed. The previous
generated app was removed after identity, signature and zero-owner checks;
engine builds still referenced by existing bottles were preserved.

Moon initially retained its title fallback. A narrow in-memory request using
the production headers returned a valid 1,352,360-byte PNG, 1024 by 1024 pixels,
and thumbnail decoding succeeded within the existing limits. That probe does
not identify the earlier request's failure or prove that every remote preview
loads reliably. No image asset or request log was retained. Narrow-window
layout and browser gameplay remain for broader user testing.

## Complete browser picture references — 9 October 2026

All 26 catalogue entries now have reviewed remote game-picture references;
the nine entries without artwork use project scenes or original game covers.
Generated coloured title cards are removed. Neutral progress/error states
remain when a remote picture cannot be obtained. Six community entries also
have explicit reviewed backup pictures, each with its own exact URL authority.

The old per-byte asynchronous body reader made a valid large Moon picture
slow to consume. The bounded chunk reader loaded it in a narrow production
probe in about 0.3 seconds. Kino's old picture returned 2,444,372 bytes, above
the unchanged 2 MiB limit; its smaller replacement loaded through the final
production loader in about 0.2 seconds. No image files were retained.

Focused checks passed: **143 Swift cover checks**, **89 existing browser and
interaction checks**, and **8 Python catalogue tests**. They cover metadata
authority, byte/image bounds, AVIF decoding, shared requests, bounded retries,
fallback exhaustion, cancellation and TLS/auth failure handling. These are
picture and interface checks, not gameplay or emulator compatibility tests.

The first candidate's visual check displayed game pictures for MW2, Skate 3,
Skate × Rust, Halo CE Mobile, Half-Life, WebXash, Hedgewars and Hextris. It also
identified Kino's oversized image and an unrelated background used by PES6's
website, which were corrected before the final package.

The final signed ARM64 package passed signature, architecture, catalogue,
DMG and checksum checks. The core hash and engine component receipt match
the preceding build. Its standard local app reopened with the existing Steam
bottle, all eight store actions and the same Home / Discover navigation.
Moon and Kino displayed their scenes; PES6 displayed the larger 16:9 original
football-game screenshot instead of the unrelated background or cropped cover.
The other added pictures were checked on screen in the preceding candidate
using the same final loader and sources. No game or installer was launched,
and graphics settings were unchanged. Caskrun was left open on Browser games.

Owned test probes, generated rollback apps and superseded candidate packages
were removed after ownership and identity checks. No game artwork was retained
on disk. Builds still referenced by real bottles were preserved. The expanded
PS5 document is a future integration plan; no emulator was installed or run.

## Collection navigation and optional PS5 imports — 9 October 2026

Home is replaced by Collection. The grid projects saved Windows apps, bounded
Steam/Epic/GOG manifest discoveries, browser bookmarks and explicit PS5 imports
into one surface. One menu filters platform and Games/Apps. Bottle management
remains available through the inspector, + menu and Command-3, including empty
or fully hidden bottles. The standalone Library/Game reports screens stay absent.

The first signed ARM64 candidate displayed the existing Steam app directly,
opened its full Windows details without entering a folder, and closed details
back to the same grid. Platform/type filters and all existing + import choices
were visible. Discover retained its eight stores and 26 reviewed browser links
with Add to Collection labels. No Windows app or game was launched.

An original non-running ELF fixture imported from an Apps filter and selected
its PS5 inspector with Open disabled because no engine was selected. Local
artwork appeared and bookmark-only removal preserved the original inputs. That
file import waited unusually long in an optional artwork open syscall. The
updated importer therefore performs no optional image IO: file imports do not
inspect siblings, and folder pictures load asynchronously with at most two
readers. Selecting an engine now selects its complete folder or app.

The oversized app-card treatment was rejected during the first visual check.
Apps now use a 48px original icon and a centred name, with selection confined to
a subtle name highlight; the duplicate subtitle and large outline/background
are removed. Game entries retain landscape picture cards. The filter menu has
no filled wrapper, and the details header preserves the complete local icon.

Focused checks passed: 32 Collection model/interaction checks, 22 local artwork
checks, 55 original PS5 metadata/process/artwork checks, and the 89 existing
browser/click checks. Full app ARM64/macOS 14 typecheck passed. PS5 fixtures use
only original compiled executables and dummy game headers; no emulator or real
game is executed. Gameplay, engine window/Dock identity, controllers and saves
remain for user testing.

The final signed package displayed the compact Steam icon/name entry and its
unclipped details icon. Details closed back to the unchanged grid. A selected
original folder fixture imported and opened its PS5 inspector in the first
post-click observation (about 0.8 seconds), with optional artwork independent
of import. The complete-engine folder picker opened and Cancel preserved the
unselected engine state. Open stayed disabled; removal cleared the inspector
and kept the original files. The temporary references and their inputs were
then removed after zero-owner checks, restoring the original absence of PS5
metadata. Caskrun was left open on Collection.

The final package passed deep/strict signature, ARM64 app/core, catalogue, DMG
and checksum checks. Its core hash and engine receipt/component bytes remain
unchanged. Superseded generated apps and packages were removed only after
identity and zero-owner checks; runtimes referenced by real bottles were kept.
No account, Windows app, browser game, PS5 engine or real game was opened in
these packaged UI checks.

The expanded Windows sidebar initially ignored wheel input even though its
native accessibility Scroll Down action moved the viewport. The shared
double-click monitor read `NSEvent.clickCount` on wheel and key events before
its pure sequence classifier could pass them through. AppKit rejects that
property on non-mouse events, as documented in
[Apple's clickCount reference](https://developer.apple.com/documentation/appkit/nsevent/clickcount).
The router now returns those original events before reading mouse properties.
Six real AppKit event regression checks passed without posting events or opening
windows. In the final signed app, wheel input moved the sidebar scrollbar from
0 to 1 and exposed Windows Tools, Files & Maintenance, Manage Bottle and the
bottom import controls. The user also confirmed scrolling works.

Inspectors now have a finite viewport, independent scroll content and
noninteractive decorative borders. A separate restart check exposed a host
icon mapping that made the core's whole-bottle ownership check refuse Quit.
Launcher and bottle icons now load bounded in-memory bytes instead of retaining
an AppKit file mapping. After that fix, the displayed Steam icon held no bottle
files and native Quit Caskrun completed normally. Core ownership checks and
engine bytes were preserved. The final app remains open for user testing.

## Compact Collection empty states — 9 October 2026

PS5 and Browser Collection no longer show a full-width grey onboarding card,
large duplicate icon or discovery/import button. A compact heading and one line
of guidance point to the existing Discover and + controls. Search and type-filter
empty results use the same quiet layout; an empty Apps/Games filter offers
Show all items rather than an unrelated import action.

The signed ARM64 package displayed both compact empty views. Selecting Apps
inside PS5 showed No apps in this view, and Show all items restored its normal
empty state. The package passed compilation, catalog, signature, architecture,
DMG and checksum checks; core and runtime receipts were unchanged. No game,
installer or Windows tool was launched, and user data was left intact. Caskrun
remained open on PS5 Collection. The previous generated app and superseded
package were removed after identity and idle checks.

## Header Add control — 9 October 2026

The floating header's Add menu now applies its 34px circular background to the
menu itself, so the native borderless control displays the intended round
button. The small plus symbol, subtle idle/hover fill and menu actions remain.

The signed ARM64 app displayed the circular control, and clicking it opened
all nine existing Add actions. Escape dismissed the menu. Compilation,
catalog, signature, architecture, DMG and checksum checks passed; core and
engine receipts were unchanged. No Windows app or game was launched. Caskrun
was left open on Discover. Superseded generated build products were removed
after identity, reference and idle checks.
