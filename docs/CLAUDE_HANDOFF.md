# Claude coding prompt

Copy the prompt below into Claude with access to this repository. The status is a handoff snapshot; inspect the current files and working tree before changing anything. Local experimental records under ignored `.build/` are intentionally absent from a fresh GitHub checkout.

---

You are continuing **Caskrun**, an original Apple Silicon macOS app that installs and runs Windows x86/x64 software through an independently built Wine runtime. It is a **private demo in active development**. More features are coming. Improve the remaining compatibility and maintenance problems through small, verified changes. Do not promise universal Windows compatibility or CrossOver parity.

## 1. Read first and preserve the product

Read `docs/ICON_COMPOSITION.md`, `AGENTS.md`, `README.md`, `docs/COMPATIBILITY_PLAN.md`, `docs/LAUNCHER_VALIDATION.md`, `docs/FEATURES.md`, `docs/LOCAL_WINE_RUNTIME.md`, `runtime-patches/README.md`, and the relevant source. Inspect `git status` and preserve pending work.

Keep these product decisions:

- Ordinary flow: Discover → Install → publisher sign-in → install a game → Play. Select the included engine and supported defaults automatically. No Wine picker or global Settings page for ordinary users.
- Home uses compact bottle folders. A single click opens the folder and its right inspector; double-click opens an app. Closing the inspector or pressing Escape returns to the bottle grid. Keep the small floating Home/Discover navigation and + menu.
- The inspector retains advanced app, display/graphics, Windows-tool and maintenance controls. Useful advanced features should not clutter ordinary installation.
- Closing Caskrun's main window keeps it available in the macOS menu bar and removes its Dock item. **Quit Caskrun** stops all Caskrun-owned Windows apps before exiting, reports unverifiable shutdown, and leaves unrelated Wine apps alone.
- Preserve the original Caskrun application and menu-bar logo. The icon work concerns Windows apps and games.
- The current icon source uses a signed ARM64 `caskrun-native-icon` helper beside the core in the app bundle, plus the same Swift algorithm in the UI for Discover and game artwork. The packaged-core original PE-artwork smoke passed first-list conversion and cache reuse without creating launch aliases or running Windows. Inspect its final code, package and verification results before editing it. Preserve the exact existing native-identity Windows runtime: icon-only changes must not trigger a bottle-engine migration. Preserve original vendor artwork, transparency, aspect ratio and quality: already complete/tiled artwork should retain its own background; sparse transparent symbols may need an appropriate native tile. Avoid double backgrounds, universal charcoal tiles, tiny artwork, invented vendor logos or a claim that Windows artwork has become the publisher's official macOS icon. Validate actual output, including missing and malformed resources. Broad publisher validation remains separate from renderer fixtures.

## 2. Boundaries and provenance

- Use original Caskrun code and independently licensed upstream components. Public product documentation can define desired behavior. Do not copy CrossOver code, recipes, compatibility databases, assets or bundled engines.
- Do not launch, alter, replace or remove an installed CrossOver or reference installation. Any further static reference inspection must be explicitly authorized by the user and respect the applicable project rules. Reference behavior is not evidence that our implementation works.
- Do not bypass trials, activation, licence checks, anti-cheat, authentication or DRM. Do not automatically accept publisher agreements or enter credentials. Ask the user to review any required agreement; continue independent work while waiting.
- Do not touch real user bottles, accounts, installed games or another Wine product. Use owned disposable fixtures. Never kill all Wine processes, trust only a saved PID, or change the host network to make a test pass.
- Keep the repository private and describe it as a demo. Do not make it public, publish a release, upload binaries or push changes without the user's publication authorization. Preserve the privacy hooks and neutral Git identity. Run the staged and revision privacy checks before any authorized commit/upload; use UTC for commits. No personal names, local absolute home paths, device/bottle/session identifiers, credentials, raw logs, screenshots, downloads or build artifacts may enter source or Git history. Never print the local private-term list.
- Caskrun's app/core are ARM64; Windows x86/x64 support remains required. Intel Mac hardware is out of scope. The independently written icon helper is MIT; Wine patches and runtime dependencies retain their own licences, notices and corresponding-source requirements. Keep packaged notices consistent with the actual source licence.

## 3. Where the implementation lives

| Area | Starting points |
| --- | --- |
| Native UI, menu/window lifecycle and core invocations | `app/CaskrunApp.swift` |
| Rust CLI, bottle metadata and launch routing | `core/src/main.rs` |
| Publisher installers and receipts | `core/src/publisher_install.rs`, `core/src/*_install.rs`, `catalog/apps.json` |
| Native names, PE artwork, cache and child identity | `core/src/native_app_identity.rs`, `core/src/app_icons.rs`, `runtime-patches/native_icon.swift`, `runtime-patches/wine-11.0-native-app-identity.patch` |
| Ownership and shutdown | `core/src/prefix_processes.rs`, `core/src/shutdown.rs`, `core/src/bottle_control.rs`, `core/src/cancellation.rs` |
| Runtime provenance and safe migration | `core/src/runtime_identity.rs`, `core/src/runtime_migration.rs`, `core/src/runtime_probe.rs` |
| Discovery, Steam games and bottle tools | `core/src/installed_apps.rs`, `core/src/steam_library.rs`, `core/src/wine_tools.rs`, `core/src/bottle_resolution.rs` |
| Import | `core/src/crossover_import.rs` |
| Isolated installer tests | `scripts/test-launchers.py`, `scripts/test_launcher_harness.py` |
| Independent runtime build/package | `scripts/build-wine-*.sh`, `scripts/package-wine-runtime-*.sh`, `runtime-patches/` |
| App/DMG and privacy tooling | `scripts/build.sh`, `scripts/package-dmg.sh`, `scripts/publish-dev-dmg.sh`, `scripts/check-publication-privacy.py` |

Native identity already derives names and checked PE icons for visible executables, hides explicit background/helper roles, uses friendly bundle names without visible cache suffixes, and provides a neutral fallback. It is experimental, not validated for every arbitrary installer/game. Generated bundles are disposable and excluded from duplicate/export/import. Preserve the bounded no-shell callbacks, file/path checks and current Steam identity/focus behavior.

Per-bottle quit and full quit share a locked, guarded stop path. A Wine server reporting idle does not prove that native clients have exited. Remaining aliases require fresh current-user/process-birth, selected-runtime and bottle-file evidence, with kernel audit-token signals and a final zero-owner check. Unknown owners remain untouched and make cleanup incomplete. Preserve these guarantees and sibling-bottle isolation.

## 4. Current launcher evidence

Read `docs/LAUNCHER_VALIDATION.md` for tested installer receipts and stage-by-stage evidence. Installer success, a saved EXE, rendered UI, authentication and gameplay are separate gates. Do not overwrite failures with “working” based on setup exit zero.

| Launcher | Actual result | Next investigation |
| --- | --- | --- |
| Epic Games Store | Checked official 20.3.3 MSI failed with an execute-access fault; no launcher saved. A separate original MSI fixture passed. | Attribute the real MSI fault with bounded MSI/Wine diagnostics, checked inputs and module/process evidence. Fixture success does not validate Epic. |
| GOG Galaxy | Setup returned zero and saved a launcher after the user accepted its agreement. The regular runtime failed service startup with error 1053/missing components, then showed a blank client. Final Dock name/icon were confirmed correct. | Reproduce the service and client separately; see the narrow prototype below. A usable client is still missing. |
| EA app | Reached the agreement prompt. The user closed it and **did not accept** the agreement. | Resume only after the user's agreement decision, then test setup and unauthenticated client UI. |
| itch.io | Setup returned zero and saved the publisher's stable bootstrap, but the Electron client was blank white. A temporary GPU-disabled attempt was also blank; actual child flag attribution was unverified. | The documented bootstrap `--` separator has now been fixed and source-tested. Identify the versioned child and its real arguments/modules before selecting a scoped rendering fix. |
| Battle.net | Installer rendered and completed; one launcher saved. Post-install and manual client launches produced no usable window. A nested exception was reported on the Wine signal stack. | Resolve executable/process/thread/module attribution and the failure path. Client icon, focus and relaunch are still unvalidated. |
| Ubisoft Connect | Setup completed and saved one launcher; post-install and manual launches produced no usable client. Logs included a nested exception and framebuffer messages without a proven causal link. | Attribute the client failure before applying dependency or graphics changes. Framebuffer output alone is not a GPU diagnosis. |
| Rockstar Games Launcher | Checked official installer started under its correct setup name. Automatic approval blocked window inspection without a further stated reason; the owned test was canceled and cleaned, with no launcher saved. | A policy block is separate from compatibility. Do not bypass it. Resume the visual/install checks through permitted tools or user observation. |

Steam previously reached a usable sign-in window with one correctly named/iconed Dock item and working focus, but other relaunches intermittently appeared black. Its observed 96-DPI/1× rendering plausibly explains a soft image; that is not proven by a high-resolution Steam test. Preserve its scoped CEF/Dock patches. Recheck normal/repeated launch, focus, high-resolution quality and full quit in a disposable Steam bottle before declaring the issue fixed. No store authentication or game has yet been validated.

### GOG: original experiment, not a production fix

The regular service bound TCP loopback but failed UDP `127.0.0.238:0` on macOS with `EADDRNOTAVAIL`; a Poco exception occurred before service dispatch. An original minimal Windows service fixture passed its lifecycle checks, so do not diagnose all Windows services as broken.

A separate original Wine prototype, retained only in an ignored reproduction record, translates **exact ephemeral IPv4 UDP** `127.0.0.238` to physical `127.0.0.1` only after that address error and only when the opt-in is exactly `CASKRUN_GOG_LOOPBACK=1`. It pairs logical address/getter/packet-metadata translation and endpoint lifetime, with a per-wineserver endpoint table. It is not general 127/8 support, fixed-port/TCP/IPv6 emulation, or host security isolation. It makes no host network change. Matching server and NTDLL/protocol components must be rebuilt together.

Ten original isolated check groups passed, including cross-process traffic, logical addresses, receive packet metadata, close/rebind, unsupported cases, invalid/absent opt-in and separate-prefix refusal. In a disposable GOG copy, service start and immediate RUNNING state succeeded, and client/service request-response worked. The client then detected a second instance, could not locate a window, exited normally and requested clean service shutdown. A fresh launch after verified shutdown still produced no usable window. Imported lock state was only a hypothesis; the service's later STOPPED state was not evidence of a crash.

Do not bundle or promote this prototype yet. Review/reproduce its independent source, regressions and network semantics first. The generalized reproduction-driver entry point was syntax-checked after the recorded run, not rerun; validate it before relying on a new result. If the ignored record is unavailable, reconstruct a fresh original reproducer from the documented behavior and upstream Wine rather than claiming access to the prior experiment.

## 5. Work in manageable phases

Start with a short evidence-based plan identifying the first concrete defect, code to change, isolated test and acceptance gate. Then implement that phase. Keep unrelated work separate and report actual progress; do not attempt every remaining feature in one patch.

### Phase A — make client failures reproducible

Choose one observed failure at a time. Use a checked current official installer in a new owned session. Record exact runtime/component identity and installer version, executable/process/thread/module attribution, launch command, exit/exception and what the user actually sees. Keep raw diagnostic output ignored and bounded. Compare normal behavior with one scoped change. Do not add universal `--disable-gpu`, sandbox disabling, random DLL overrides or broad redistributable bundles. Apply a workaround only when controlled evidence supports it, with a narrow version/profile scope and regression check.

For GOG, separate the loopback barrier from singleton/window restoration. For itch, separate the stable bootstrap from its versioned client. For Battle.net/Ubisoft, identify the nested exception instead of guessing from an adjacent framebuffer warning. For Epic, preserve actual MSI evidence.

### Phase B — reliable original install profiles

Build versioned profiles with architecture/Windows/runtime requirements, verified publisher/component sources, ordered dependencies, per-step checks/receipts, cancellation and restart recovery. Resume completed steps safely; preserve unrelated bottles. Detect an installed product and validate its first launch separately. Determine prerequisites from current publisher/upstream evidence and controlled tests. Add installed-component inventory/repair and honest actionable failure states. A downloaded installer or existing EXE alone is insufficient.

### Phase C — real graphics and display support

Create a runtime capability manifest for the exact packaged engine. Use it for apps, children, installers, tools and reboot. Only offer supported choices, with mutually exclusive explicit bottle backends and tested original automatic profiles. Preserve user DLLs/overrides; require idleness, verification and rollback for changes.

- WineD3D is the current path, with limited probes and no game validation.
- High-resolution controls really write 96/192 DPI and Retina values with idle/rollback checks; validate Steam and other client output, not just the DPI fixture.
- DXMT is a sensible next independent Direct3D experiment. Its upstream Wine `winemac` API integration is required; dropping DLLs into the bottle is insufficient. Pin sources/licences and test x86/x64 device creation, shaders/readback, visible frames, shutdown/relaunch and a selected real game.
- DXVK/MoltenVK has only Vulkan enumeration evidence and failed Direct3D gates. Resolve required capabilities before exposing a working backend choice.
- D3DMetal needs an independent source/terms/distribution review and runtime tests; never extract another app's copy.
- MSync is not implemented by an environment toggle. It needs coordinated client/server support and correctness/performance tests.
- DLSS/MetalFX needs supported backend/vendor-extension integration and per-game checks.
- The current Wine build lacks GStreamer. Evaluate independently sourced media support with codec/playback tests; do not assume this explains current launcher failures.

Do not add cosmetic toggles for unsupported features. Keep status planned/experimental/validated accurate.

### Phase D — finish bottle and command workflows

- Preserve complete shortcut commands: checked executable, correct Windows arguments and working directory. Current discovery refuses argument-bearing `.lnk` files instead of silently launching a partial command.
- Improve portable handling of Wine-created host-folder links without following external links or copying host data. Current export rejects absolute links; do not weaken this into unrestricted copying.
- Add explicit icon-cache rebuild/repair, installed-component inventory, bounded diagnostic channels/export, integrity-checked compressed backups and reviewed runtime/profile updates.
- Check existing rename, duplicate, import, delete/restore, Windows tools, controllers and shutdown through the real UI. Do not reimplement an existing feature from an outdated status row.
- Keep session reboot's warning that it closes apps; prove upstream semantics before adding a less disruptive alternative.
- Test import into a new independent bottle; leave the source untouched. Existing CrossOver import supports idle win64 folders, excludes proprietary metadata/engine files and external mappings, and has only synthetic validation. `.cxarchive` and 32-bit-only imports are unsupported.

## 6. Verification and cleanup gates

Use `scripts/test-launchers.py`; each session isolates Caskrun storage, HOME mappings, temporary files and XDG paths. Backend tests do not require the SwiftUI window, but they do not validate the Home/Discover UI journey.

For each supported launcher or manual app, validate: checked publisher receipt → user-reviewed setup → usable unauthenticated UI → correct native name/icon → one appropriate Dock identity for each visible app, no background helper entries → focus after another app is in front → quit/relaunch → stop the owned bottle → verified zero owners. Test authentication, downloads, controllers and gameplay separately with the user's chosen accounts/titles. Include arbitrary manually installed programs, visible child apps/games and background services.

Run relevant meaningful tests and required source/package checks after each change. Before a broader handoff, run the serial Rust suite and applicable harness/catalog/privacy checks, format/diff checks and build verification. The current baseline passed **260 unit tests, 2 integration tests, 46 Python checks and 23 real icon CLI cases**, with 2 Rust tests ignored; parallel runs exposed timing sensitivity. Refresh counts from the current code. Do not conceal ignored, failing, unrun or blocked checks. These results do not establish vendor compatibility.

Keep builds, downloads, logs, fixtures and caches in owned ignored locations. Never replace an app/runtime serving live processes. Use separate candidate output and verify signatures, DMG/hash and source/licence receipts. Cleanup only exact owned test data after bottle shutdown, idle-server and whole-session zero-owner checks. If cleanup is uncertain, preserve the fixture and report it. Retain only useful original reproduction source and sanitized outcomes, then remove disposable receipts/artifacts. Do not glob-delete unknown directories.

## 7. Deliverables

Finish the selected phase with original code, appropriate tests, clean owned test cleanup and updated compatibility/feature documentation. Report the defect, verified change, remaining failures, blocked checks and next smallest step. Mark only evidence-supported stages as validated. Keep Caskrun simple and polished while compatibility develops; do not publish or claim a release-ready app.
