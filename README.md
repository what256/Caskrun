# Caskrun

**Demo — in active development.** Caskrun is still in the demo phase. More features and improvements are coming; Windows apps and games are not guaranteed to work yet. The repository is private during this early development phase.

Caskrun is a new, independent Apple Silicon macOS app for managing Wine bottles and installing Windows software. It is **not affiliated with CodeWeavers** and contains no CrossOver application code or assets. The name is provisional pending a publication trademark check. Windows x86/x64 apps remain a compatibility target on Apple Silicon; Intel Mac hardware is outside the project scope.

The Caskrun app and core compile only for ARM64. The first Wine compatibility experiment uses an x86_64 Wine process through Rosetta **on an Apple Silicon Mac**; this does not add Intel Mac support.

## Current demo

- Native SwiftUI desktop interface.
- Rust core for bottle storage, Wine runtime selection, prefix initialization with `wineboot`, and launching a chosen `.exe` or `.msi` in a bottle.
- Searchable, file-backed catalog with eight experimental automatic install actions: Steam, Epic Games Store, GOG Galaxy, Battle.net, Ubisoft Connect, EA app, itch.io app, and Rockstar Games Launcher. Failed setups retain a per-bottle receipt and installer, whose format and SHA-256 are rechecked before a same-bottle retry. All app compatibility remains marked untested.
- Custom installer flow for any local `.exe` or `.msi`. Caskrun creates a bottle automatically; installing into an existing bottle is available from its inspector on Home. After the direct installer exits successfully, Caskrun suggests newly found Windows Start Menu/Desktop shortcuts or Program Files executables for explicit addition to Home, with a manual EXE picker fallback. The installer can leave child processes running, and an EXE's presence does not prove that setup or compatibility succeeded.
- Manual and catalog setups have a Cancel action. It stops the download or direct Wine process owned by that setup, preserves a retryable catalog receipt when one exists, and leaves other bottles alone. Wine-spawned descendants can remain; the packaged Cancel button still needs a live UI check.
- Bottle renaming, duplication, recoverable deletion/restore, portable folder backup/restore, C: drive access in Finder, and saved launchers that can be hidden from Home without uninstalling them. The right inspector on Home can search for installed programs, run an EXE, open the bottle's drive and logs, quit Windows apps, and open Wine's configuration and diagnostic tools. App Options provides temporary and saved launch arguments, plus Hide/Show on Home. Manage Bottle contains rename, duplicate, export, and recoverable deletion; New bottle, Import, and Recently Deleted remain available in the inspector. A per-bottle High Resolution Mode switch is available only while its apps are stopped.
- An included independent Wine 11.0 development engine in the local DMG. The app selects it automatically; there is no Wine picker or Settings screen. A version check does not establish Windows app compatibility.
- Per-launch logs in each bottle, with a bounded log viewer and Copy action in the app.
- Experimental per-app native Dock names and adaptive macOS artwork from checked Windows executable resources, including a fallback that keeps the friendly name. Service and explicit Chromium/Electron helper roles are hidden; generated native bundles stay in disposable bottle caches. A shared adaptive renderer preserves existing publisher backgrounds, normalizes padding and supports bare marks without double tiles; see [icon composition](docs/ICON_COMPOSITION.md). A user-led original test-window check confirmed one named Dock icon and focus. Vendor app validation remains separate; see [launcher validation](docs/LAUNCHER_VALIDATION.md).
- Home and Discover use a small floating navigation capsule with a + menu. Home lists all active bottles as compact folder tiles, including empty bottles and those with hidden launchers. One click opens a folder's smaller app/game icons and right bottle inspector; clicking an app or game selects it immediately, while double-click, Open, or Command-O launches it. Icons remain still on hover. The inspector preserves bottle, app, display, Windows-tool, file, and maintenance controls; its close button or Escape returns to all bottle folders. The large heading blocks are removed from Home and Discover, and Command-F reveals an optional toolbar search. Discover groups game stores and publisher launchers into compact rows with clear Install, Open, Retry, and Resume actions.
- Installed Steam games are discovered from local app manifests and bottle-local C: libraries when a folder opens, every 15 seconds while it remains open, and when Caskrun regains focus. Games use locally cached original artwork when available, with a generic fallback, and launch through the bottle's saved Steam app. Discovery and launch routing have fixture tests; real gameplay remains unverified.
- Closing the main window or launching a saved Windows app keeps Caskrun available under its logo and name in the macOS menu bar while its Dock icon disappears. **Quit Caskrun** now waits for all Caskrun-owned bottles to stop their Windows apps before exiting. It cancels owned setup invocations, waits for other active operations to settle, blocks new launches, and keeps the UI open with an error if shutdown cannot be verified. This uses Wine's per-bottle stop control; it does not stop other Wine products. The menu offers saved apps, Open Caskrun, Home, Discover, bottle controls, file installation, About, and Quit. Earlier user-led checks confirmed the menu item and window reopening. The current packaged smoke verified sidebar x/Escape returning to all bottles and full Quit stopping the running Steam main process and its four web helpers before Caskrun exited. Failed menu launches reopen the window to show the error. Discover fetches publisher artwork; saved apps use their own executable icons.
- Wine's Desktop and native shortcut locations are isolated inside each Caskrun bottle. A fresh real-Wine bottle created a private Desktop mapping, and the packaged app migrated the existing Caskrun Steam bottle's exact legacy Desktop link on launch without creating another host Desktop shortcut. Automatic shortcut conversion still needs a complete test.

**The local development DMG includes the Windows engine.** Its native app and core are ARM64; its independent x86_64 Wine 11.0 engine runs through Rosetta and includes project-built FreeType/GnuTLS plus pinned Wine Mono/Gecko. Caskrun selects the included engine without asking for a Wine path. The bundled engine passed `wine --version` and fresh-bottle initialization through the ARM64 core; a self-authored MSI installed through the separate development package. Caskrun rejects Wine paths inside CrossOver or CrossOver-Lab.

The Steam recipe downloaded Valve's installer, created a bottle and launcher, and updated to the sign-in stage. An earlier packaged build showed a black sign-in window and two Wine Dock entries. In a disposable test, an independently patched Wine 11.0 runtime rendered the sign-in window and gave its web helper a single Steam Dock identity with working focus. The current local DMG now bundles that source-built runtime. On first launch, Caskrun migrated only the existing verified Steam bottle from the exact previous runtime to the exact new one, retaining its original runtime record. In an earlier packaged build, the Home icon launched Steam and its web helpers; the intended CEF flags appeared on the helper command line and Steam logged `WaitingForCredentials`. User-led checks then found a usable sign-in window and one Steam Dock icon that restored focus, although other relaunches briefly appeared black and the window looked low-resolution. The Steam bottle remains at 96 DPI, and its observed renderer used a 1× display scale on a Retina panel; this plausibly explains the soft image, but a 192-DPI Steam test has not been run. Steam's original installer stalled twice in the user-data bottle; guarded same-bottle recovery succeeded from the core, but the SwiftUI retry action has not been tested. See [Steam results](docs/STEAM_SMOKE.md).

The October isolated launcher round found an Epic MSI execute-access fault and a GOG component/service failure after setup returned zero and saved a launcher. GOG's native Dock name and artwork were confirmed, but its reopened window was blank; diagnostics traced its service failure to a loopback socket bind. Battle.net and Ubisoft setup completed and saved launchers, but their client launches produced no usable windows. itch.io setup completed, but its client window was blank. The EA agreement was not accepted; its test was closed. Rockstar setup started, but automatic approval blocked window inspection; the disposable test was canceled and cleaned. **Store authentication, games, graphics, and controller behavior remain unverified.** See the stage-by-stage [launcher validation record](docs/LAUNCHER_VALIDATION.md) and [compatibility implementation plan](docs/COMPATIBILITY_PLAN.md). This ad-hoc signed DMG is a local experiment, not a distributable release; see [runtime notes](docs/LOCAL_WINE_RUNTIME.md), [Epic results](docs/EPIC_SMOKE.md), and [feature gaps](docs/FEATURE_GAPS.md).

Bottle backups use an inspectable `.caskrun-bottle` directory with a manifest and payload. They are not compressed, checksummed, or an atomic snapshot yet. Duplicate/export refuse tracked live processes and, for initialized prefixes, wait for the prefix-specific Wine server to become idle. This requires the selected independent Wine runtime to match the bottle's recorded engine, including an identical relocated bundled engine. Close all apps in the bottle before exporting it; launchers that point outside the bottle may need to be selected again after import.

The + menu also offers **Import from CrossOver** for an idle, existing **win64 bottle folder** containing `drive_c` and `system.reg`. It copies installed apps and data into a new Caskrun bottle without changing the source, excluding CrossOver metadata and engine files, the Windows tree, builtin programs, source symlinks, and external drive mappings. Dependencies may need repair or reinstallation under Caskrun's engine. `.cxarchive` files and 32-bit-only prefixes are unsupported. Synthetic import tests passed; real imported-app compatibility and the import UI have not been verified.

Moving a bottle to Recently Deleted keeps its files in Caskrun's own data directory so it can be restored. For an initialized bottle, Caskrun refuses the move if its Wine server is still active or a matching independent runtime cannot be checked. Restore does not require Wine. Permanent removal and automatic cleanup are not implemented yet.

## What is coming

- More reliable store installation, recovery, and launcher updates.
- Broader app and game validation on Apple Silicon.
- Graphics and controller improvements as independent runtimes support them.
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

The current serial Rust verification passed **260 unit tests and 2 integration tests, with 2 existing tests ignored**. Parallel runs exposed timing-sensitive failures; this is a serial verification result. The Python harness, privacy and catalog checks passed **46 tests**. An earlier Foundation-only harness passed **126 checks** of the async quit reply and shutdown command gate. These results cover core behavior and fixtures. Earlier packaged checks verified folder exit, Discover/search, and full Quit with running Steam; real imported-app and game compatibility remain unverified. See [launcher validation](docs/LAUNCHER_VALIDATION.md) for the current live test outcomes.

## Design and scope

See [docs/FEATURES.md](docs/FEATURES.md) for the complete feature inventory and honest progress status. [assets/caskrun-icon.png](assets/caskrun-icon.png) uses the original Caskrun symbol, adapted with imagegen into a white mark on a charcoal rounded tile with transparent outside corners. It appears as the Finder and Dock app icon; [assets/caskrun-menubar-template.png](assets/caskrun-menubar-template.png) is a transparent monochrome cutout of that symbol for the macOS menu bar. The window has Home and Discover in a floating capsule with a right bottle inspector. Its + menu offers Discover apps, Install from file, New bottle, Import Caskrun backup, Import from CrossOver, and Manage bottles. File installation targets the open Home folder when one is selected. Home uses folder navigation and smaller app/game icons; the right inspector keeps every saved launcher, including hidden ones, in its App picker and retains App Options, Display & Graphics, Windows Tools, Files & Maintenance, and Manage Bottle. Manage bottles or Command-3 opens those controls, including Recently Deleted. Discover groups compact rows under Game stores and Publisher launchers, with direct Install/Open/Resume/Retry actions. Command-F reveals search in the capsule. [UI validation](docs/UI_SMOKE.md) records earlier layouts; the current game and import flows still need broader live checks. The build script converts the master image into the app's `.icns` sizes.

Caskrun itself is available under MIT (`LICENSE`). Wine and other runtime components keep their own licenses; their binaries are excluded from this source repository and are included only in the local development DMG with notices and source references.


## Contributing and source license

Caskrun's original app, core, catalog, and build tooling are MIT-licensed. The Wine patches in `runtime-patches/` are LGPL-2.1-or-later; see [third-party notices](THIRD_PARTY_NOTICES.md). This source repository excludes local builds, runtimes, installers, bottles, logs, and release images. No demo binary is distributed through GitHub yet.

Read [CONTRIBUTING.md](CONTRIBUTING.md) before committing. The local privacy checks protect staged files and outgoing Git history; local author identity and private match terms stay in `.git/`.
