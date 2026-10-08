# Launcher validation workflow

Caskrun is a demo in active development. More features are coming, and the repository remains private. An installer download, a saved executable, or a returned process ID does not prove launcher or game compatibility.

## Library and graphics phase

_8 October 2026. These additions do not change the real publisher-client outcomes recorded below. Authentication and gameplay remain untested._

- **Local Library:** bounded read-only discovery lists installed Steam games,
  Epic `.item` manifests and GOG game metadata inside supported bottle-local
  C: locations, alongside saved Windows shortcuts. Original fixtures exercise
  identity, path containment, malformed records, limits and read-only behavior.
  Steam launches through its saved client; Epic/GOG entries open their saved
  store client. Full account-owned libraries, arbitrary external drives and
  verified per-game launch commands are not provided by manifest discovery.
- **External compatibility evidence:** an attributed offline CC0 snapshot
  contains 373 Steam identities and 71 direct reports. The UI compares known
  generic device and runtime fields while retaining missing values, mixed
  reports and renderer-availability uncertainty. All entries remain **Not
  tested in Caskrun**. An external rating or matching chip does not establish
  a result under Caskrun's engine. See [data boundaries](GAME_COMPATIBILITY.md).
- **Steam installation requests:** a selected report or manually supplied
  Steam AppID/store link can target an existing saved Steam client or a fresh
  Steam setup. Input and routing checks do not establish a completed download.
  Steam handles account sign-in, ownership and the actual game installation;
  the user will test that journey with their chosen account and games.
- **Browser shortcuts:** Discover offers checked developer/community links
  and custom HTTPS links. Saved links open in the default browser from Home
  or Library without Wine. Tests cover local persistence, deduplication,
  removal and refused credential-bearing or unsafe links. Website inspection
  does not establish gameplay or future availability. The Vice City community
  port requires the user's own game files; Caskrun includes no game assets.
  See [browser provenance](../catalog/BROWSER_GAMES.md).
- **Optional DXMT controls:** the original live matrix passed five control
  combinations under both Windows x64 and x86: 30/60 presentation targets
  with spatial scaling off, plus application-controlled presentation at
  1.25×/1.5×/2× spatial output scale. Each passed the recorded drawing,
  readback, two-window lifecycle, selected-runtime and busy-refusal gates.
  The owned prefixes were stopped, independently checked and removed.
  API-submission timings are not displayed FPS or game-performance results.
  Visible image quality, fullscreen, real games and other Macs remain manual
  tests. See [exact DXMT evidence](DXMT_OPTIONS.md).

Automatic remains WineD3D. High Resolution stays optional and off by default;
DXMT frame targets and spatial scaling are optional and default off. These
controls add neither DLSS, temporal reconstruction, frame generation, D3D12,
DXVK, D3DMetal nor MSync. No performance advantage over CrossOver is claimed.

## Isolated local tests

Use `scripts/test-launchers.py` with a Caskrun core and independent Wine runtime under the ignored `.build` directory. Each session owns its own Caskrun storage, macOS home-folder mappings, temporary files and XDG directories. Keep real accounts and user bottles out of these tests.

```sh
python3 scripts/test-launchers.py setup --core <built-core> --wine <runtime-bin-wine>
python3 scripts/test-launchers.py install <session> <catalog-id>
python3 scripts/test-launchers.py status <session>
python3 scripts/test-launchers.py run <session> <catalog-id>
python3 scripts/test-launchers.py record <session> <catalog-id> --result rendered
python3 scripts/test-launchers.py cleanup <session>
```

Installation and window inspection are separate steps. The harness does not accept publisher agreements or enter account credentials. A rendered sign-in window still leaves authentication and gameplay untested. Record failures and blocked steps explicitly. Use `cancel <session>` to request cancellation through that invocation's stdin; cleanup checks the entire owned test session after stopping its bottles.

Cleanup refuses to remove data while a core, tracked descendant, Wine server or other process still holds test files. Resolve that owner first, then retry. Do not send global Wine signals or delete unrelated prefixes. Raw diagnostics stay in the disposable ignored session; only a small sanitized receipt survives cleanup. Remove that receipt after transferring the necessary non-personal result into this document.

The harness exercises Caskrun's Rust backend directly. The SwiftUI window does not have to be open for an isolated installer test. This verifies backend and Windows-app behavior; it does not substitute for testing the same journey through Home or Discover.

## Acceptance checklist

For each launcher: verify the current official installer source and checked file receipt; complete setup with any publisher agreement reviewed and accepted by the user; confirm a usable unauthenticated window; verify the saved launcher's name and native icon; move another app in front and click its Dock icon; quit, reopen, then stop the bottle; prove test processes and files are removed. Test account authentication and games separately with the user's chosen accounts and titles.

Include a manually installed Windows app and its child process in native identity checks. Background services must not create extra Dock entries; visible applications need their own recognizable name, icon and focus behavior.

## Native app identity and shutdown

The independent development runtime now has an opt-in native identity path for Windows apps. Caskrun derives a friendly name and vendor artwork from the selected executable's checked PE resources, with saved launcher names taking precedence. A shared original adaptive renderer preserves finished publisher shapes, normalizes padding and adds contrast support only for sparse marks. Home prepares image-only artwork before launch, and the same policy produces native and Retina Dock sizes. See [icon composition](ICON_COMPOSITION.md). Missing or invalid artwork uses an original fallback while preserving the app's name. Caskrun's own app and menu-bar logos are unchanged.

Each visible executable gets a generated native app bundle in its bottle. Its parent directory carries the internal cache key; the visible app name has no cache suffix. Bounded, stable file checks restrict this path to the current bottle, the selected runtime's builtin tools, or an explicitly chosen initial installer. Visible child applications can receive their own identity; service processes and explicit Chromium/Electron helper roles are hidden from the Dock. Home reuses validated cached artwork without creating native bundles during listing. These bundles are disposable and excluded from bottle duplicate, export and import payloads.

Generic installer metadata such as “Setup/Uninstall” no longer overrides a specific product name. A checked running catalog receipt can also provide the name for its own installer and recognized temporary installer child. This contextual label stops applying when that setup is no longer running.

A verified idle Wine server is not sufficient if client processes still hold bottle files. After ordinary per-bottle shutdown, the core can stop remaining native aliases only when fresh evidence ties them to the locked current bottle and selected runtime. It checks the current user, process birth, loader and mapped runtime files, plus bottle file ownership, and sends signals through the kernel's process audit token. Changed or uncertain evidence leaves the process alone and reports incomplete shutdown. Unrelated Wine products and unverified global Wine processes are detection-only. Cleanup still requires a final idle-server and zero-owner check before deleting test data.

The explicit Quit Windows apps action now shares the same server/native-owner/zero-owner path as full Caskrun quit, with a continuous bottle lock and prefix guard. Original native TERM/KILL tests verify that a sibling bottle remains untouched.

These are implemented safeguards, not a promise that every Windows app has correct naming, rendering or shutdown behavior. Broader publisher, manually installed app and child-game checks remain necessary.

## Current test round

_7 October 2026; Apple Silicon, independent Wine 11.0. Authentication and gameplay are untested._

The initial diagnostic round used the earlier Steam-enabled engine. Its four installer tests were stopped and their isolated prefixes, downloads and logs removed. Later tests use the new general native-identity runtime in separate disposable sessions. The diagnostic prefixes, downloads and logs from this round were subsequently removed after their owners were stopped and cleanup was verified. Reproducible original source experiments and build products remain in the ignored development area.

| Launcher | Observed evidence | Remaining acceptance stage |
| --- | --- | --- |
| Epic Games Store | Initial round: checked official 20.3.3 MSI downloaded; interactive MSI exited with an execute-access fault. No launcher saved. | Diagnose setup failure; see [Epic smoke](EPIC_SMOKE.md). |
| GOG Galaxy | Current native-runtime round: the user reviewed and accepted the licence; setup returned zero and saved one launcher. First launch reported missing essential components and a GalaxyClientService start failure (1053). Reopen showed a blank white window. Diagnostics traced a service startup exception to binding a second loopback address; see below. The user confirmed correct artwork and a final Dock hover name of just “GOG Galaxy,” without a cache suffix. Per-bottle quit and zero-owner checks passed. | Investigate loopback compatibility, then prove a usable launcher, relaunch and Dock focus. No production GOG fix or host-network change has been applied. Both the original diagnostic bottle and its disposable prototype copy were subsequently stopped and removed after zero-owner checks; only abstract outcomes are retained. |
| EA app | Initial round: installer reached its agreement prompt. The user closed it; the agreement was **not accepted**. | Agreement decision, setup and launcher checks. |
| itch.io | Current native-runtime round: checked official setup completed with exit zero and saved its stable bootstrap launcher. The client window was blank white. A temporary GPU-disabled launch also showed a blank window; the child flag was not independently attributed, so this does not establish a GPU diagnosis. Per-bottle quit and zero-owner checks passed. | Verify bootstrap argument forwarding and identify the versioned client before diagnosing rendering. The owned diagnostic fixture was subsequently removed after zero-owner checks. |
| Battle.net | Current native-runtime round: the checked official installer rendered, completed with exit zero and saved one launcher. English (US) was selected and automatic startup was disabled. Neither the post-install launch nor a manual relaunch produced a usable client window. The initial Wine log reported a nested exception on the signal stack. Per-bottle quit completed; the harness verified zero owners and removed the test fixture. | Diagnose the missing client window. Client name, icon, Dock focus and usable relaunch remain untested. |
| Ubisoft Connect | Current native-runtime round: checked official setup completed with exit zero and saved one launcher. Automatic startup in the wizard was disabled; the catalog handoff and a manual relaunch produced no usable client window. Logs include a nested exception and framebuffer errors; their causal relationship is unproven. Per-bottle quit and zero-owner checks passed. | Attribute the client failure to a process/module. Client Dock focus and usable relaunch remain untested. The owned diagnostic fixture was subsequently removed after zero-owner checks. |
| Rockstar Games Launcher | Current checked official installer started with the native name “Rockstar Games Launcher Setup.” Automatic approval blocked inspecting its window without a further stated reason. The harness canceled the test; no launcher was saved. Quit completed, zero-owner checks passed and the owned fixture was removed. | Installer interaction, usable client, artwork, focus and relaunch remain blocked/unverified. |

The current GOG installer was 342,291,968 bytes, with SHA-256 `ec1ef3744a6ba365c022d5e114a482b74ff0496e00a91924ebdae401e2c9291b`, obtained through its checked official manifest and publisher CDN. The receipt identifies the tested bytes; setup exit zero and an executable on disk do not establish a usable Galaxy client.

The GOG service successfully bound TCP `127.0.0.1:9978`, then failed to bind UDP `127.0.0.238:0` with macOS `EADDRNOTAVAIL`. A Poco network exception terminated it before it entered the Windows service dispatcher. This narrows the observed failure to that service startup path; it does not establish a repair or exclude additional Galaxy compatibility problems. An original opt-in prototype subsequently passed ten isolated check groups for exact ephemeral IPv4 UDP alias handling, logical addresses, packet metadata, endpoint lifetime, unsupported protocols, invalid/absent opt-in and separate-prefix refusal. All original fixture sessions were removed after zero-owner checks. In a disposable GOG copy, service start returned zero and an immediate query showed RUNNING; the client then detected a second instance, could not locate its window, exited normally and explicitly requested service shutdown. The service stopped cleanly. Another launch after verified complete bottle shutdown still produced no usable window. This experiment does not establish stable GOG compatibility and is not included in the regular engine. The host network was unchanged.

An original minimal Win32 x64 service passed dispatch, service-main entry, running and stopped states under the same native runtime. Its `sc create`, `start`, `query`, `stop` and `delete` checks passed. The fixture and its receipt were removed after zero-owner cleanup. This establishes basic service lifecycle support for that original fixture; Galaxy's failure is not evidence that every Windows service is unsupported.

The Battle.net installer was 4,896,464 bytes, with SHA-256 `de5d32d4ea5eed5a9e120027fb68b370976dbfecc8f2a8f91305977f0b87fcaf`. Its checked receipt matched the downloaded bytes. Installer rendering and completion passed, but neither launch attempt established a usable client.

The Ubisoft installer was 264,197,496 bytes, with SHA-256 `3cba64c512523f58579f83c10c30d55004ef59a7f926a67903fe1fa5ddf2539a`. The itch.io setup was 18,168,600 bytes, with SHA-256 `4b239be6e2aafdb6e1ce22dea16d264381bb8632e8c3d00fb2fd1302682fee87`. Their checked receipts matched the tested downloads. Neither setup completion established a usable client.

The user-directed static reference review produced an [original implementation plan](COMPATIBILITY_PLAN.md) for setup profiles, runtime capabilities, graphics integration and bottle/command improvements. Reference behavior does not replace these launcher tests.

The Rockstar installer was 111,419,512 bytes, with SHA-256 `321886457b7e4ab727a27671c17a815684a3943579eedf7e2248a3b9f0973746`; its checked receipt matched the tested download. Setup was canceled after the window-inspection block, so it has no completed-install result.

The earlier GOG setup initially used the generic “Setup/Uninstall” name. Contextual installer naming corrected it to “GOG Galaxy Setup.” A later native-bundle layout change removed the letters-and-numbers suffix visible in the Dock. These naming checks are separate from the unresolved launcher failure.

An original Caskrun native test window passed a user-led check of one correctly named Dock icon and bringing the window back after Finder was in front. That establishes the native identity and focus mechanism for that fixture; it does not establish focus or compatibility for every publisher app. The test's processes and files were removed after ownership checks.

Initial GOG tests exposed orphaned clients holding test files after Wine reported its server idle. The harness preserved the session rather than deleting active data. Those initial sessions were subsequently cleaned. A later live GOG test exercised the new scoped native-client stop path and verified complete shutdown with no remaining file owners.

A separate original Caskrun MSI installed its exact test payload successfully. This establishes basic MSI execution for that fixture only. Neither a source check nor a successful test fixture establishes vendor-launcher compatibility.

## Steam: rendering, Dock identity, relaunch and shutdown

_7 October 2026; Apple Silicon, independent Wine 11.0 native-identity engine inside a signed candidate app, isolated `test-launchers.py` session. No account was created or signed in to, and no game was installed._

Steam installed through the ordinary catalog path: setup returned zero, `Steam.exe` was saved, one launcher named "Steam" was registered, and the checked installer receipt matched the downloaded bytes. Measurements below come from two original offscreen tools that read window metadata and captured window pixels; both were removed afterwards.

**Rendering is not black.** Across seven launches in this round, every one produced a visible "Sign in to Steam" window. The fraction of near-black pixels was 0.000 in every capture. The earlier report of intermittently black relaunches was not reproduced with this engine.

**Rendering was blurry, and high-resolution mode fixes it.** Sharpness was measured as the ratio of pixel variation inside each 2x2 block to variation across block boundaries on a 2x capture: content genuinely drawn at 2x varies inside each block, while content drawn at 1x and upscaled is flat inside it. Two native Retina applications scored 0.974 and 1.001 as controls.

| Bottle state | `LogPixels` | `RetinaMode` | Sharpness ratio |
| --- | --- | --- | --- |
| Default | 96 | unset | **0.000** (drawn at 1x, upscaled) |
| High resolution on | 192 | `y` | **0.801 – 0.826** across five sign-in windows |

The window stayed 700x440 points in both states, so the control sharpens rendering without resizing the client. This validates the existing per-bottle High Resolution Mode on a real publisher client for the first time; it is still off by default, and no game has been measured, so defaulting it on is not justified by this evidence.

**One correctly named Dock entry.** While the sign-in window was open, exactly one Caskrun Windows process had the regular activation policy that places an icon in the Dock, and it was named "Steam". The remaining Steam processes, including `Steam.exe` itself and its helpers, were accessory or prohibited, so no background helper appeared in the Dock.

**Focus works.** With Finder brought to the front, activating the Steam application the way its Dock icon does made Steam frontmost and active with its window on screen.

**Shutdown works.** With Steam running, the shared quit path reported `complete: true` with one bottle stopped and no failures. Afterwards no Steam window, no Caskrun Windows application and no process holding a test file remained. Two unrelated CrossOver Wine servers were running before and after and were never signalled.

Authentication, downloads, controllers and gameplay remain untested, as does Direct3D. This round establishes installer, rendering, identity, focus and shutdown behaviour for Steam's unauthenticated client only.

## High resolution beyond Steam: an engine default, and a client-side limit

_8 October 2026; same engine, signed candidate app and isolated session; measured with the same sharpness ratio and removed afterwards._

The Steam result above left one question open: whether 1x rendering is specific to Steam or an engine-wide default. It is engine-wide, and enabling the control is not sufficient on its own.

| Window | Bottle state | Sharpness ratio |
| --- | --- | --- |
| Wine Configuration (engine builtin) | default | **0.011** — drawn at 1x, upscaled |
| Wine Configuration (engine builtin) | high resolution on | **0.994** — matches native Retina applications |
| Steam sign-in (DPI-aware client) | high resolution on | 0.801 – 0.826 |
| itch.io Setup (publisher installer) | high resolution on | **0.027** — still drawn at 1x |

Wine's own builtin window is upscaled by default and becomes as sharp as a native application once the control is on, so the blurriness is an engine default rather than a Steam problem. The itch.io installer, however, stayed at 1x in a bottle where the control was on and had just been shown to work. High resolution sets the Mac driver's Retina backing store and a 192 DPI value; a client that does not scale with that DPI value keeps drawing at 1x into the larger backing store and is still upscaled. The control therefore sharpens DPI-aware clients only, which is a property of each client and not something a bottle setting can force.

This strengthens the case for enabling high resolution by default, because the engine's own windows benefit and no client was made worse. It does not settle it: the setting applies to a whole bottle, and no game has been measured at either setting.

The itch.io client itself was not reached. Its setup window rendered under the correct native name "itch.io app Setup", then stayed on its first screen for about nine minutes without progressing, and the attempt was cancelled. An earlier attempt in the same round was cancelled by the test tooling rather than by the installer, so neither attempt is evidence about itch.io's own behaviour. Its bootstrap, versioned child and reported blank window remain unresolved.

## Bottle copy and backup with Wine's user folders

_7 October 2026; Apple Silicon, independent Wine 11.0 native-identity runtime. No vendor launcher was installed for this check._

A disposable bottle initialized by the real runtime showed that Wine gives its account absolute links to this macOS account's `Documents`, `Downloads`, `Music` and `Pictures` folders, to `Videos` as `Movies`, and — when the account has a `Templates` folder — to a seventh link nested at `AppData/Roaming/Microsoft/Windows/Templates`. Caskrun's own private Desktop mapping was already handled. Because the backup copier refused any other absolute link, both **Duplicate bottle** and **Export bottle** failed on every initialized bottle with `Backup contains an absolute symlink at prefix/drive_c/users/<account>/Music`. That was a complete outage of both actions for real bottles, not an edge case.

Copy, export and import now record each of Wine's user-folder mappings as a folder name and recreate it for the account that owns the new bottle. One explicit standard-folder table decides what may be recorded and what may be restored, so both directions refuse the same things.

A disposable two-account check with the independent runtime produced the following. A bottle created and initialized under the first account received all six mappings, including the nested Templates path. It duplicated successfully, and exported with six recorded mappings, a recorded Windows profile folder, and **no file in the backup referencing the account's home folder**. Restoring it into the same account relinked all six and needed no profile link; a Windows `cmd.exe` run read the installed program's `%APPDATA%\ExampleApp\settings.ini` from the restored profile and wrote it through `%USERPROFILE%\Documents` into the real mapped folder. Restoring the same backup into a second account whose home had only `Documents` and `Pictures` relinked those two, kept the other four as real folders inside the bottle, and linked that account's Windows profile to the restored one; the same Windows check read the restored profile's settings and wrote into the second account's own `Documents`. Nothing was created in either home folder, and `wineboot` left the restored layout and the profile link unchanged. The signed candidate app's packaged core repeated the create, initialize, duplicate, export and cross-account restore sequence with the same results.

Policy refusals were checked live against a real bottle. A `Documents` link pointing at `~/.ssh` or `~/Library`, a non-standard `Favorites` link pointing at `~/Documents`, and a `Documents` link pointing at the wrong standard folder each stopped the export with `Backup contains an absolute symlink at …` and left no backup behind. A `Desktop` link pointing at the real `~/Desktop` is converted to the bottle's own private Desktop before the copy, which is why Desktop is absent from the table and is never carried as a mapping.

Cross-account restores previously left the bottle's original Windows account folder unreachable, so `%USERPROFILE%` pointed at a new, nearly empty folder `wineboot` created and imported programs lost their settings. Each backup now records the profile folder it holds, and a restore reports whether this account can reach it rather than assuming it can. A five-account disposable session checked every outcome with the independent runtime, and the signed candidate app's packaged core repeated the conflicting-link, fresh-link and refusal cases:

| Restored into | Outcome | Evidence |
| --- | --- | --- |
| An account with no profile folder in the bottle | resolved, link added | `AccountTwo -> AccountOne`; a Windows program read the recorded profile's settings |
| The account that owns the recorded profile | resolved, no link needed | no warning reported |
| An account whose payload already carries the correct link | resolved, no second link written | exporting a connected bottle recorded the profile it resolves to, and re-importing recognised it |
| An account that already owns a profile folder | **not** resolved, warning returned | its own settings file and the recorded profile both survived unchanged |
| An account whose profile link points elsewhere | **not** resolved, warning returned | the existing link was kept and the recorded profile left intact |
| A manifest recording a profile absent from the payload | refused | `Backup is missing the Windows profile folder it records`, and no bottle was created |

Caskrun states that imported programs keep their settings and data only when the profile resolved; otherwise it shows the warning. Nothing is renamed, moved, merged or removed in any of these cases. Backups are written as `.caskrun-bottle` version 2; a version 1 backup carries neither mappings nor a profile and still restores, while a version 1 manifest claiming either field and any other version are refused. All owned test sessions were stopped, checked for remaining owners and removed.

## Source verification

The earlier profile-restoration round passed 267 Rust unit tests and 2 integration tests, with 2 existing tests ignored. Parallel runs exposed timing-sensitive failures, so that serial result was not a clean parallel-suite result. Its Python harness, publication-privacy and catalog checks passed 47 tests; the native UI typechecked and a signed candidate app verified its signature. The subsequent maintenance phase passed 375 Rust unit tests and 6 integration tests, with 2 existing tests ignored, plus 97 Python checks and 10 Swift argument checks. The library phase adds focused browser, compatibility-data, publisher-manifest and Steam-request checks; its graphics matrix is recorded separately above. Earlier adaptive-renderer image cases and packaged-core artwork checks retain their original scope. These tests cover source behavior, API probes and fixtures, not real store authentication or games.

See the separate [feature inventory](FEATURES.md) for the demo's limitations.


## Library phase packaged checks — 8 October 2026

The current source passes 420 Rust unit tests and 6 integration tests, with 2
existing unit tests ignored, 125 Python checks and 104 Swift checks. The full
SwiftUI app typechecks. The complete Rust suite passes with one test thread;
an earlier heavily parallel run exceeded mock-process deadlines. Those tests
passed without relaxing their bounds. Clippy reports existing style warnings
in unchanged files; the new feature code introduces no remaining warnings.

An owned fixture exercised the packaged core's browser-link add/list/rename/
remove commands, installed Steam/Epic/GOG game discovery and saved-client
recognition. The read commands left the synthetic prefix byte-for-byte
unchanged and never initialized or launched Wine. The fixture was removed
after independent zero-file-owner inspection. These are metadata and command
checks, not publisher account or game compatibility results.

The final packaged core also passed the combined 60 FPS / 1.5× DXMT trial on
both Windows architectures and the packaged executable-artwork smoke. All
three owned fixtures were removed after zero-owner checks. The interface-only
device capability gate preserves the tested core and runtime bytes.

The new Library visual pass remains pending because the Mac was locked. Real
publisher sign-in, browser gameplay and actual games remain manual tests; see
[the checklist](MANUAL_TEST_CHECKLIST.md). DXMT rendering evidence and its
explicit limits are recorded in [DXMT options](DXMT_OPTIONS.md).
