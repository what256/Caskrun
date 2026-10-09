# Caskrun

**Demo — in active development.** Caskrun is still in the demo phase. More features and improvements are coming; Windows apps and games are not guaranteed to work yet. The repository is private during this early development phase.

Caskrun is a new, independent Apple Silicon macOS app for collecting games and apps, managing Wine bottles and installing Windows software. It is **not affiliated with CodeWeavers** and contains no CrossOver application code or assets. The name is provisional pending a publication trademark check. Windows x86/x64 apps remain a compatibility target on Apple Silicon; Intel Mac hardware is outside the project scope.

The Caskrun app and core compile only for ARM64. The first Wine compatibility experiment uses an x86_64 Wine process through Rosetta **on an Apple Silicon Mac**; this does not add Intel Mac support.

## Current demo

- Native SwiftUI desktop interface.
- Rust core for bottle storage, Wine runtime selection, prefix initialization with `wineboot`, and launching a chosen `.exe` or `.msi` in a bottle.
- Searchable, file-backed catalog with eight experimental automatic install actions: Steam, Epic Games Store, GOG Galaxy, Battle.net, Ubisoft Connect, EA app, itch.io app, and Rockstar Games Launcher. Failed setups retain a per-bottle receipt and installer, whose format and SHA-256 are rechecked before a same-bottle retry. All app compatibility remains marked untested.
- Custom installer flow for any local `.exe` or `.msi`. Caskrun creates a bottle automatically; installing into an existing bottle is available from its inspector on Collection. After the direct installer exits successfully, Caskrun suggests newly found Windows Start Menu/Desktop shortcuts or Program Files executables for explicit addition to Collection, with a manual EXE picker fallback. The installer can leave child processes running, and an EXE's presence does not prove that setup or compatibility succeeded.
- Manual and catalog setups have a Cancel action. It stops the download or direct Wine process owned by that setup, preserves a retryable catalog receipt when one exists, and leaves other bottles alone. Wine-spawned descendants can remain; the packaged Cancel button still needs a live UI check.
- Bottle renaming, duplication, recoverable deletion/restore, compressed integrity-checked backup/restore with older folder-backup support, C: drive access in Finder, and saved launchers that can be hidden from Collection without uninstalling them. The right inspector on Collection can search for installed programs, run an EXE, open the bottle's drive and logs, quit Windows apps, and open Wine's configuration and diagnostic tools. App Options provides temporary and saved launch arguments, plus Hide/Show on Collection. Manage Bottle contains rename, duplicate, export, and recoverable deletion; New bottle, Import, and Recently Deleted remain available in the inspector. A per-bottle High Resolution Mode switch is available only while its apps are stopped.
- An included independent Wine 11.0 development engine in the local DMG. The app selects it automatically; there is no Wine picker or Settings screen. A version check does not establish Windows app compatibility.
- Per-launch logs with bounded Copy/Save Text, privacy-limited local diagnostic reports, installed-program inventory, recoverable app-icon rebuilding, bottle notes and optional Windows-version control. App renaming and literal one-time arguments are available in the inspector. Discovered shortcuts preserve checked C: working folders through launch, duplicate and backup. See [maintenance workflows](docs/DEMO_MAINTENANCE.md) and the [manual checklist](docs/MANUAL_TEST_CHECKLIST.md).
- Experimental per-app native Dock names and adaptive macOS artwork from checked Windows executable resources, including a fallback that keeps the friendly name. Service and explicit Chromium/Electron helper roles are hidden; generated native bundles stay in disposable bottle caches. A shared adaptive renderer preserves existing publisher backgrounds, normalizes padding and supports bare marks without double tiles; see [icon composition](docs/ICON_COMPOSITION.md). A user-led original test-window check confirmed one named Dock icon and focus. Vendor app validation remains separate; see [launcher validation](docs/LAUNCHER_VALIDATION.md).
- **Collection / Discover** share the small floating navigation capsule and + menu. Collection is a flat grid of saved Windows apps, installed Steam/Epic/GOG games, browser bookmarks and explicitly imported PS5 games. One filter menu switches All, Windows, PS5 or Browser and Games or Apps. A click opens details without entering a bottle folder; double-click or Command-O opens the item. The right panel keeps Windows graphics, tools and bottle controls. Discover remains the place to add stores and browser games; there is no separate Library, Game reports or Settings screen.
- Read-only Steam, Epic and GOG manifest readers populate Collection automatically while it is visible and when the app becomes active. Discovery is bounded and restricted to each bottle’s real C: drive; it does not sign in or synchronize an account’s full owned library. Steam games launch through their saved Steam client. Epic/GOG game cards open their saved store, rather than claiming a direct game launch. Presence on disk does not establish compatibility. See [library boundaries](docs/GAME_COMPATIBILITY.md).
- The + menu can request installation using a Steam store link or AppID through an existing saved Steam client or a new Steam bottle. The user signs in, owns the game and completes installation inside Steam; Caskrun does not import credentials or bypass ownership. The reviewed community snapshot remains in source, but the compatibility browser is no longer exposed. See [compatibility evidence](docs/GAME_COMPATIBILITY.md).
- Discover offers 26 browser-game and collection links, each with a reviewed game picture. Original game covers are used where a community port has no suitable scene preview. Known saved links reuse their pictures on Collection. Picture downloads are shared and retry transient failures; loading or failed requests no longer show coloured title cards. Community ports, own-file requirements and unverified links are labelled. Games open in the default browser; custom public HTTPS bookmarks stay local and do not trigger artwork crawling. Caskrun does not bundle commercial game assets or run websites in Wine. Gameplay remains untested. See [browser links and provenance](catalog/BROWSER_GAMES.md) and [future emulator research](docs/EMULATION_RESEARCH.md).
- **Experimental PS5 adapter:** import your own game folder, ELF or ZAR file without moving it, then explicitly attach a complete local KytyPS5 Mac folder or app. Imports and the engine record stay in private local metadata, separate from Wine bottles. Caskrun checks supported Mach-O architecture, Rosetta prerequisites and a matching local MoltenVK library, then uses the documented argument-array launch. Imported pictures come from local `icon0.png` when available. Focus, bounded logs, Stop and full Quit track only processes Caskrun started. Original fixture checks cover metadata and process behavior; no emulator or PS5 game has been launched. AnyPS5 conversion is still research, not an installed feature. See [PS5 scope and remaining checks](docs/EMULATION_RESEARCH.md).
- Optional DXMT controls provide an application-controlled or bounded frame target and MetalFX spatial output scaling. The original x64/x86 probe matrix passed on the development Mac, including renderer ownership, drawing, readback, window lifecycle and refused edits while running. Those API checks do not establish displayed game FPS, image quality or a performance advantage. Automatic still selects WineD3D; High Resolution stays optional and off by default. See [DXMT options and remaining tests](docs/DXMT_OPTIONS.md).
- Closing the main window keeps Caskrun available in the macOS menu bar. **Quit Caskrun** waits for Caskrun-owned Windows apps and experimental PS5 engine processes to stop, refuses to exit when shutdown cannot be verified, and leaves unrelated processes alone. Browser tabs belong to the browser and stay open. The menu offers saved Windows apps, Open Caskrun, Collection, Discover, bottle controls, file installation, About and Quit. Earlier Wine shutdown and menu checks are recorded separately from the new PS5 fixture checks.
- Wine's Desktop and native shortcut locations are isolated inside each Caskrun bottle. A fresh real-Wine bottle created a private Desktop mapping, and the packaged app migrated the existing Caskrun Steam bottle's exact legacy Desktop link on launch without creating another host Desktop shortcut. Automatic shortcut conversion still needs a complete test.

**The local development DMG includes the Windows engine.** Its native app and core are ARM64; its independent x86_64 Wine 11.0 engine runs through Rosetta and includes project-built FreeType/GnuTLS plus pinned Wine Mono/Gecko. Caskrun selects the included engine without asking for a Wine path. The bundled engine passed `wine --version` and fresh-bottle initialization through the ARM64 core; a self-authored MSI installed through the separate development package. Caskrun rejects Wine paths inside CrossOver or CrossOver-Lab.

The Steam recipe downloaded Valve's installer, created a bottle and launcher, and updated to the sign-in stage. An earlier packaged build showed a black sign-in window and two Wine Dock entries. In a disposable test, an independently patched Wine 11.0 runtime rendered the sign-in window and gave its web helper a single Steam Dock identity with working focus. The current local DMG now bundles that source-built runtime. On first launch, Caskrun migrated only the existing verified Steam bottle from the exact previous runtime to the exact new one, retaining its original runtime record. In an earlier packaged build, the Collection icon launched Steam and its web helpers; the intended CEF flags appeared on the helper command line and Steam logged `WaitingForCredentials`. User-led checks then found a usable sign-in window and one Steam Dock icon that restored focus, although other relaunches briefly appeared black and the window looked low-resolution. Later isolated measurements confirmed that High Resolution sharpens DPI-aware Steam and Wine builtin windows, while a DPI-unaware itch.io installer remains upscaled. High Resolution remains optional and off by default; game performance is untested. See [display measurements](docs/LAUNCHER_VALIDATION.md). Steam's original installer stalled twice in the user-data bottle; guarded same-bottle recovery succeeded from the core, but the SwiftUI retry action has not been tested. See [Steam results](docs/STEAM_SMOKE.md).

The October isolated launcher round found an Epic MSI execute-access fault and a GOG component/service failure after setup returned zero and saved a launcher. GOG's native Dock name and artwork were confirmed, but its reopened window was blank; diagnostics traced its service failure to a loopback socket bind. Battle.net and Ubisoft setup completed and saved launchers, but their client launches produced no usable windows. itch.io setup completed, but its client window was blank. The EA agreement was not accepted; its test was closed. Rockstar setup started, but automatic approval blocked window inspection; the disposable test was canceled and cleaned. **Store authentication, gameplay, real-game graphics and controller behavior remain unverified.** See the stage-by-stage [launcher validation record](docs/LAUNCHER_VALIDATION.md) and [compatibility implementation plan](docs/COMPATIBILITY_PLAN.md). This ad-hoc signed DMG is a local experiment, not a distributable release; see [runtime notes](docs/LOCAL_WINE_RUNTIME.md), [Epic results](docs/EPIC_SMOKE.md), and [feature gaps](docs/FEATURE_GAPS.md).

New bottle backups use a compressed `.caskrun-backup` archive with a checked entry manifest and per-file SHA-256 integrity. Older inspectable `.caskrun-bottle` folder backups remain supported. Import validates the payload before creating a bottle; export publishes a complete new archive without overwriting an existing file. Duplicate/export require stopped bottle apps and verified prefix ownership. Close all apps before exporting; launchers pointing outside the bottle may need to be selected again after import. Browser bookmarks are separate local app data and are not included in bottle backups.

The + menu also offers **Import from CrossOver** for an idle, existing **win64 bottle folder** containing `drive_c` and `system.reg`. It copies installed apps and data into a new Caskrun bottle without changing the source, excluding CrossOver metadata and engine files, the Windows tree, builtin programs, source symlinks, and external drive mappings. Dependencies may need repair or reinstallation under Caskrun's engine. `.cxarchive` files and 32-bit-only prefixes are unsupported. Synthetic import tests passed; real imported-app compatibility and the import UI have not been verified.

Moving a bottle to Recently Deleted keeps its files in Caskrun's own data directory so it can be restored. For an initialized bottle, Caskrun refuses the move if its Wine server is still active or a matching independent runtime cannot be checked. Restore does not require Wine. Permanent removal and automatic cleanup are not implemented yet.

## What is coming

- More reliable store installation, recovery, and launcher updates.
- Broader app and game validation on Apple Silicon.
- Real-game tests for DXMT, its optional frame target and spatial scaling; other graphics backends and controller improvements as independent runtimes support them.
- Better backups, app updates, clean-machine testing, and release packaging.

These are development goals, without a promised release date. See [feature gaps](docs/FEATURE_GAPS.md) for current limits and [the feature inventory](docs/FEATURES.md) for implemented and planned behavior.

## Build

Requires an Apple Silicon Mac, macOS 14 or later, Xcode command-line tools, Rust/Cargo, and the `aarch64-apple-darwin` Rust target.

```sh
./scripts/build.sh
open .build/Caskrun.app
```

The build stays in `.build/`; the script does not install into `/Applications`.
Builds refuse to replace a Caskrun app while its own process or bundled Wine runtime is in use. To prepare a separate app while Steam keeps running from `.build/Caskrun.app`, set `CASKRUN_BUILD_OUTPUT_DIR="$PWD/.build/candidate"` for `build.sh` or `CASKRUN_PACKAGE_OUTPUT_DIR="$PWD/.build/candidate"` for `package-dmg.sh`.

For a local development DMG, first stage the independent native Wine base and build the reviewed switchable package described in [DXMT app integration](docs/DXMT_APP_INTEGRATION.md). The default packager includes that package; WineD3D remains the default renderer and DXMT is an optional experimental bottle control. Package into an isolated build directory, then publish the verified image to `dist/`:

```sh
mkdir -p .build/release-candidate
CASKRUN_PACKAGE_OUTPUT_DIR="$PWD/.build/release-candidate" ./scripts/package-dmg.sh
./scripts/publish-dev-dmg.sh .build/release-candidate
```

Packaging rebuilds the ARM64 app, includes the runtime and its source/license notices, and verifies the image. Publishing verifies the candidate and any existing deliverable before replacing the visible DMG and SHA-256 file; it does not replace an app bundle serving a running Wine process. The DMG is ad-hoc signed and **not notarized for public distribution**. `./scripts/build.sh` alone creates a lightweight app without a bundled engine.

The published local development image is at `dist/Caskrun-0.1.0-arm64-dev.dmg`, with its SHA-256 file beside it. The `dist/` folder contains local deliverables and is not committed to Git or uploaded as a release.

```sh
(cd dist && shasum -a 256 -c Caskrun-0.1.0-arm64-dev.dmg.sha256)
```

To test the core without affecting your real bottles:

```sh
cd core
cargo test
CASKRUN_HOME="$(mktemp -d)" cargo run -- list
```

The current source passes **420 Rust unit tests and 6 integration tests, with 2 existing tests ignored**, plus **125 Python checks** and **104 Swift checks**. These cover browser URL privacy and persistence, compatibility evidence, installed-game manifests, Steam installation requests and the existing maintenance workflows. The complete SwiftUI app typecheck also passes. Its original DXMT matrix separately exercises frame targets and spatial scales under both Windows architectures; see the [recorded evidence](docs/DXMT_OPTIONS.md). Source, API and fixture tests do not establish broad publisher-client or real-game compatibility. See [maintenance workflows](docs/DEMO_MAINTENANCE.md), [manual testing](docs/MANUAL_TEST_CHECKLIST.md), and [launcher validation](docs/LAUNCHER_VALIDATION.md).

## Design and scope

See [docs/FEATURES.md](docs/FEATURES.md) for the feature inventory and validation limits. [assets/caskrun-icon.png](assets/caskrun-icon.png) retains the original Caskrun mark for Finder and Dock; [assets/caskrun-menubar-template.png](assets/caskrun-menubar-template.png) is its monochrome menu-bar cutout. The floating capsule has Collection, Discover and a + menu. Command-F reveals search; Command-3 opens bottle management. [UI validation](docs/UI_SMOKE.md) records each layout and its checks. The build script generates the app’s `.icns` sizes.

Caskrun itself is available under MIT (`LICENSE`). Wine and other runtime components keep their own licenses; their binaries are excluded from this source repository and are included only in the local development DMG with notices and source references.


## Contributing and source license

Caskrun's original app, core, catalog metadata and build tooling are MIT-licensed. The separately attributed Highball compatibility snapshot is CC0-1.0; see [data provenance](docs/GAME_COMPATIBILITY.md) and its bundled licence. The Wine patches in `runtime-patches/` are LGPL-2.1-or-later; see [third-party notices](THIRD_PARTY_NOTICES.md). This source repository excludes local builds, runtimes, installers, bottles, logs, and release images. No demo binary is distributed through GitHub yet.

Read [CONTRIBUTING.md](CONTRIBUTING.md) before committing. The local privacy checks protect staged files and outgoing Git history; local author identity and private match terms stay in `.git/`.
