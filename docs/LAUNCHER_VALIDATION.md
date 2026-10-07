# Launcher validation workflow

Caskrun is a demo in active development. More features are coming, and the repository remains private. An installer download, a saved executable, or a returned process ID does not prove launcher or game compatibility.

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

The independent development runtime now has an opt-in native identity path for Windows apps. Caskrun derives a friendly name and vendor artwork from the selected executable's checked PE resources, with saved launcher names taking precedence. An original renderer prepares a padded macOS icon tile and Retina sizes. Missing or invalid artwork uses an original fallback while preserving the app's name. Caskrun's own app and menu-bar logos are unchanged.

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

## Source verification

The current serial Rust verification passed 250 unit tests and 2 integration tests; 2 existing tests remain ignored. Parallel runs exposed timing-sensitive failures, so the serial result is not a clean parallel-suite result. The Python harness, publication-privacy and catalog checks passed 46 tests. These tests cover source behavior and fixtures, not real store authentication or games.

See the separate [feature inventory](FEATURES.md) for the demo's limitations.
